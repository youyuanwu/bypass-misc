//! Async networking with DPDK and smoltcp
//!
//! This module provides async/await support for TCP connections using DPDK
//! as the underlying packet I/O layer and smoltcp for the TCP/IP stack.
//!
//! # Architecture
//!
//! ## Runtime Abstraction
//!
//! This module is runtime-agnostic via the [`Runtime`] trait. A [`TokioRuntime`]
//! implementation is provided for tokio. To use a different runtime, implement
//! the `Runtime` trait.
//!
//! The user must:
//! 1. Create an async runtime (e.g., tokio `current_thread`)
//! 2. Spawn the reactor's `run()` method as a background task
//! 3. Use `TcpStream` and `TcpListener` normally in async code
//!
//! ## DPDK is Poll-Based
//!
//! Unlike interrupt-driven systems (tokio with epoll), DPDK requires continuous
//! polling - there are no interrupts to notify us when packets arrive.
//! The `Reactor::run()` method polls DPDK in a loop.
//!
//! ## How Wakers Work
//!
//! 1. **Reactor polls DPDK + smoltcp** continuously in a background task
//! 2. **Socket futures register wakers** with smoltcp when they would block
//! 3. **smoltcp wakes those wakers** when socket state changes during poll
//! 4. **Tokio schedules those tasks** to run
//!
//! # Example
//!
//! ```no_run
//! use dpdk_net::tcp::{DpdkDeviceWithPool, Reactor, TcpListener, TcpStream};
//! use smoltcp::iface::Interface;
//! use smoltcp::wire::IpAddress;
//! use tokio::runtime::Builder;
//!
//! fn example(device: DpdkDeviceWithPool, iface: Interface) {
//!     // Create single-threaded tokio runtime
//!     let rt = Builder::new_current_thread().enable_all().build().unwrap();
//!
//!     rt.block_on(async {
//!         // Create reactor with DPDK device and smoltcp interface
//!         let reactor = Reactor::new(device, iface);
//!         let handle = reactor.handle();
//!
//!         // Spawn the reactor polling task (runs forever)
//!         tokio::task::spawn_local(async move {
//!             reactor.run().await;
//!         });
//!
//!         // Create a listening socket
//!         let mut listener = TcpListener::bind(&handle, 8080, 4096, 4096)
//!             .expect("bind failed");
//!
//!         // Accept and handle connections
//!         let stream = listener.accept().await.expect("accept failed");
//!
//!         let mut buf = [0u8; 1024];
//!         let n = stream.recv(&mut buf).await.expect("recv failed");
//!         stream.send(&buf[..n]).await.expect("send failed");
//!     });
//! }
//! ```

mod runtime;
mod socket;
#[cfg(feature = "tokio")]
pub mod tokio_compat;

pub use runtime::Runtime;
pub use socket::{
    AcceptFuture, CloseFuture, TcpListener, TcpRecvFuture, TcpSendFuture, TcpStream,
    WaitConnectedFuture,
};
#[cfg(feature = "tokio")]
pub use tokio_compat::{TokioRuntime, TokioTcpStream};

// Re-export smoltcp error types for convenience
pub use smoltcp::socket::tcp::{ConnectError, ListenError};

use super::DpdkDeviceWithPool;
use smoltcp::iface::{Interface, PollIngressSingleResult, PollResult, SocketHandle, SocketSet};
use smoltcp::phy::Device;
use smoltcp::time::Instant;
use std::cell::RefCell;
use std::rc::Rc;

/// Default number of packets to process before yielding to other tasks.
/// This balances responsiveness with throughput.
const DEFAULT_INGRESS_BATCH_SIZE: usize = 32;

/// Shared state for the async reactor
///
/// This holds all the smoltcp state and provides interior mutability
/// so that futures can access it.
///
/// Wakers are managed by smoltcp's socket API directly via
/// `register_recv_waker()` and `register_send_waker()`.
pub struct ReactorInner<D: Device> {
    pub device: D,
    pub iface: Interface,
    pub sockets: SocketSet<'static>,
    /// Orphaned sockets that are in graceful close but no longer owned by a TcpStream.
    /// These will be cleaned up once they reach Closed or TimeWait state.
    pub(crate) orphaned_closing: Vec<SocketHandle>,
}

impl<D: Device> ReactorInner<D> {
    /// Process one incoming packet (bounded work).
    ///
    /// Returns whether a packet was processed and whether socket state changed.
    fn poll_ingress_single(&mut self, timestamp: Instant) -> PollIngressSingleResult {
        let ReactorInner {
            device,
            iface,
            sockets,
            ..
        } = self;
        iface.poll_ingress_single(timestamp, device, sockets)
    }

    /// Transmit queued packets (bounded work).
    /// Returns whether any socket state changed.
    fn poll_egress(&mut self, timestamp: Instant) -> PollResult {
        let ReactorInner {
            device,
            iface,
            sockets,
            ..
        } = self;
        iface.poll_egress(timestamp, device, sockets)
    }

    /// Clean up orphaned sockets that have completed their graceful close.
    ///
    /// Sockets in TimeWait or Closed state can be safely removed.
    fn cleanup_orphaned(&mut self) {
        use smoltcp::socket::tcp::State;

        self.orphaned_closing.retain(|&handle| {
            let socket = self.sockets.get::<smoltcp::socket::tcp::Socket>(handle);
            match socket.state() {
                State::Closed | State::TimeWait => {
                    // Socket is fully closed, remove it
                    self.sockets.remove(handle);
                    false // Remove from orphan list
                }
                _ => true, // Keep in orphan list, still closing
            }
        });
    }
}

impl<D: Device> Drop for ReactorInner<D> {
    fn drop(&mut self) {
        use smoltcp::socket::tcp::State;

        // Final cleanup pass - remove any sockets that completed closing
        self.orphaned_closing.retain(|&handle| {
            let socket = self.sockets.get::<smoltcp::socket::tcp::Socket>(handle);
            match socket.state() {
                State::Closed | State::TimeWait => {
                    self.sockets.remove(handle);
                    false
                }
                _ => true,
            }
        });

        if !self.orphaned_closing.is_empty() {
            // Count remaining sockets by state for debugging
            let (mut fin_wait1, mut fin_wait2, mut closing, mut last_ack, mut other) =
                (0, 0, 0, 0, 0);

            for &handle in &self.orphaned_closing {
                let socket = self.sockets.get::<smoltcp::socket::tcp::Socket>(handle);
                match socket.state() {
                    State::FinWait1 => fin_wait1 += 1,
                    State::FinWait2 => fin_wait2 += 1,
                    State::Closing => closing += 1,
                    State::LastAck => last_ack += 1,
                    _ => other += 1,
                }
            }

            tracing::info!(
                orphaned_sockets = self.orphaned_closing.len(),
                fin_wait1,
                fin_wait2,
                closing,
                last_ack,
                other,
                "Reactor shutting down with orphaned sockets still closing"
            );
        }
    }
}

/// The async reactor that drives DPDK + smoltcp
///
/// This must be polled repeatedly to make progress on network I/O.
/// Use with tokio's single-threaded runtime (`current_thread`).
pub struct Reactor<D: Device> {
    inner: Rc<RefCell<ReactorInner<D>>>,
}

impl Reactor<DpdkDeviceWithPool> {
    /// Create a new reactor with the given DPDK device and interface
    pub fn new(device: DpdkDeviceWithPool, iface: Interface) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ReactorInner {
                device,
                iface,
                sockets: SocketSet::new(vec![]),
                orphaned_closing: Vec::new(),
            })),
        }
    }

    /// Get a handle to the reactor's inner state (for creating sockets)
    pub fn handle(&self) -> ReactorHandle {
        ReactorHandle {
            inner: self.inner.clone(),
        }
    }

    /// Run the reactor forever using tokio, polling DPDK continuously with bounded work.
    ///
    /// This is a convenience method equivalent to `run_with::<TokioRuntime>()`.
    /// It should be spawned as a background task using `tokio::task::spawn_local`.
    ///
    /// To avoid DoS from packet floods, this uses `poll_ingress_single()` to process
    /// packets in batches, yielding between batches. This ensures that even under
    /// heavy load, other async tasks get a chance to run.
    ///
    /// Uses the default batch size of 32 packets. For custom batch sizes, use
    /// [`run_with_batch_size`](Self::run_with_batch_size).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dpdk_net::tcp::{DpdkDeviceWithPool, Reactor};
    /// # use smoltcp::iface::Interface;
    /// # async fn example(device: DpdkDeviceWithPool, iface: Interface) {
    /// let reactor = Reactor::new(device, iface);
    /// let handle = reactor.handle();
    ///
    /// // Spawn reactor as background task
    /// tokio::task::spawn_local(async move {
    ///     reactor.run().await;
    /// });
    ///
    /// // Now use handle to create sockets...
    /// # }
    /// ```
    #[cfg(feature = "tokio")]
    pub async fn run(self) -> ! {
        self.run_with::<TokioRuntime>(DEFAULT_INGRESS_BATCH_SIZE)
            .await
    }

    /// Run the reactor with tokio and a custom ingress batch size.
    ///
    /// This is a convenience method equivalent to `run_with::<TokioRuntime>(batch_size)`.
    ///
    /// `batch_size` limits how many ingress packets are processed before running
    /// egress. This prevents DoS attacks where RX floods could starve TX,
    /// causing the server to never send responses.
    ///
    /// Recommended values:
    /// - 16-32: Good balance for mixed workloads
    /// - 64-128: High-throughput scenarios
    /// - 1-8: When latency for other tasks is critical
    #[cfg(feature = "tokio")]
    pub async fn run_with_batch_size(self, batch_size: usize) -> ! {
        self.run_with::<TokioRuntime>(batch_size).await
    }

    /// Run the reactor with a custom async runtime.
    ///
    /// This is the most flexible run method, allowing you to use any runtime
    /// that implements the [`Runtime`] trait.
    ///
    /// # Type Parameters
    ///
    /// * `R` - The runtime implementation to use for yielding
    ///
    /// # Arguments
    ///
    /// * `batch_size` - Maximum number of ingress packets to process before
    ///   running egress. This prevents DoS attacks where a flood of incoming
    ///   packets could starve egress, causing the server to never send responses
    ///   (ACKs, SYN-ACKs, data). Without this limit, an attacker could prevent
    ///   any outbound traffic by saturating the RX queue.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dpdk_net::tcp::{DpdkDeviceWithPool, Reactor};
    /// # use dpdk_net::tcp::async_net::TokioRuntime;
    /// # use smoltcp::iface::Interface;
    /// # async fn example(device: DpdkDeviceWithPool, iface: Interface) {
    /// let reactor = Reactor::new(device, iface);
    ///
    /// // Run with explicit runtime and batch size
    /// reactor.run_with::<TokioRuntime>(64).await;
    /// # }
    /// ```
    pub async fn run_with<R: Runtime>(self, batch_size: usize) -> ! {
        let mut iterations_since_yield = 0usize;

        loop {
            let timestamp = Instant::now();
            let mut packets_processed = 0;

            // Process ingress in batches
            loop {
                let result = {
                    let mut inner = self.inner.borrow_mut();
                    inner.poll_ingress_single(timestamp)
                };

                match result {
                    PollIngressSingleResult::None => break,
                    _ => {
                        packets_processed += 1;
                        if packets_processed >= batch_size {
                            // Hit batch limit - break to run egress before continuing
                            break;
                        }
                    }
                }
            }

            // Check shared ARP cache and inject any new entries before egress.
            // This is critical for multi-queue setups where ARP replies go to queue 0
            // but TCP connections are on other queues. The injected ARP packets need
            // to be processed by smoltcp for the neighbor cache to be populated.
            {
                let mut inner = self.inner.borrow_mut();
                inner.device.inject_from_shared_cache();

                // If we injected any packets, run a mini ingress pass to process them
                // so the neighbor cache is populated before egress tries to send.
                if !inner.device.rx_batch_is_empty() {
                    // Process all injected ARP packets
                    while inner.poll_ingress_single(Instant::now()) != PollIngressSingleResult::None
                    {
                        // Continue until all injected packets are processed
                    }
                }
            }

            // Process egress - transmit queued packets
            // Loop egress multiple times to give all sockets a fair chance.
            // smoltcp iterates sockets in order and breaks when TX is full,
            // so we flush TX and retry to let later sockets send too.
            // Limit iterations to avoid spinning if NIC TX ring is full.
            {
                let mut inner = self.inner.borrow_mut();

                const MAX_EGRESS_ROUNDS: usize = 4;
                for _ in 0..MAX_EGRESS_ROUNDS {
                    // Try to flush any pending TX packets to make room
                    inner.device.flush_tx();

                    // Poll egress with fresh timestamp for accurate timer processing
                    let result = inner.poll_egress(Instant::now());

                    // If no socket had anything to send, we're done
                    if result == PollResult::None {
                        break;
                    }

                    // If TX has room, all sockets got a chance - done
                    if !inner.device.is_tx_full() {
                        break;
                    }
                    // TX full, loop back to flush and retry for fairness
                }
                // Final flush to push any remaining packets
                inner.device.flush_tx();
            }

            // Clean up orphaned closing sockets that have completed their handshake
            {
                let mut inner = self.inner.borrow_mut();
                inner.cleanup_orphaned();
            }

            // Check TX headroom before deciding whether to yield
            let should_yield = {
                let inner = self.inner.borrow();
                // Only yield when TX has at least half capacity available.
                // This ensures tasks have room to queue packets when they wake.
                // If TX is more than half full, keep looping to:
                // 1. Drain RX packets (prevent drops)
                // 2. Process ACKs (which free TX buffer space in smoltcp)
                // 3. Give the NIC time to transmit while we do useful work
                inner.device.tx_available() >= inner.device.tx_capacity() / 2
            };

            iterations_since_yield += 1;

            // Yield if TX has headroom OR we've gone too long without yielding.
            // The latter ensures accept/recv tasks get a chance to poll even under load.
            if should_yield || iterations_since_yield >= 16 {
                iterations_since_yield = 0;
                // Yield to let other async tasks run (accept handlers, recv futures, etc.)
                R::yield_now().await;
            }
        }
    }
}

/// Handle to the reactor for creating sockets
#[derive(Clone)]
pub struct ReactorHandle {
    pub(crate) inner: Rc<RefCell<ReactorInner<DpdkDeviceWithPool>>>,
}
