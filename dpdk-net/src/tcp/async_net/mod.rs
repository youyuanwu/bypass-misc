//! Async networking with DPDK and smoltcp
//!
//! This module provides async/await support for TCP connections using DPDK
//! as the underlying packet I/O layer and smoltcp for the TCP/IP stack.
//!
//! # Architecture
//!
//! ## DPDK is Poll-Based
//!
//! Unlike interrupt-driven systems (tokio with epoll, embassy with interrupts),
//! DPDK requires continuous polling - there are no interrupts to notify us
//! when packets arrive. This means we must always poll DPDK for packets.
//!
//! ## How Wakers Work Here
//!
//! Despite DPDK's poll-based nature, we still use wakers effectively:
//!
//! 1. **Reactor polls DPDK + smoltcp** continuously (required for packet I/O)
//! 2. **Socket futures register wakers** with smoltcp when they would block
//! 3. **smoltcp wakes those wakers** when socket state changes during poll
//! 4. **Executor only polls futures** when their waker was triggered
//!
//! This means we always poll DPDK, but we avoid redundant future polls
//! when sockets haven't changed state.
//!
//! ## Comparison with Embassy
//!
//! Embassy's architecture is similar but interrupt-driven:
//! - `Runner::run()` is a task that polls smoltcp when woken
//! - Socket operations call `waker.wake()` to trigger a poll
//! - Embedded network drivers can use interrupts to wake the runner
//!
//! Our architecture is poll-driven:
//! - `block_on()` continuously polls DPDK (no interrupts available)
//! - smoltcp wakes socket wakers during poll when state changes
//! - Futures are only polled when their waker was triggered
//!
/// # Example
///
/// ```no_run
/// use dpdk_net::tcp::{DpdkDeviceWithPool, Reactor, TcpListener, TcpStream};
/// use smoltcp::iface::Interface;
/// use smoltcp::wire::IpAddress;
///
/// fn example(device: DpdkDeviceWithPool, iface: Interface) {
///     // Create reactor with DPDK device and smoltcp interface
///     let reactor = Reactor::new(device, iface);
///     let handle = reactor.handle();
///
///     // Create a listening socket (like std::net::TcpListener::bind)
///     let mut listener = TcpListener::bind(&handle, 8080, 4096, 4096)
///         .expect("bind failed");
///
///     // Run async code with the reactor
///     reactor.block_on(async {
///         // Accept a connection (like std::net::TcpListener::accept)
///         // The listener remains valid and can accept more connections
///         let stream = listener.accept().await.expect("accept failed");
///
///         // Echo received data
///         let mut buf = [0u8; 1024];
///         let n = stream.recv(&mut buf).await.expect("recv failed");
///         stream.send(&buf[..n]).await.expect("send failed");
///     });
/// }
/// ```
mod socket;
mod waker;

pub use socket::{
    AcceptFuture, TcpListener, TcpRecvFuture, TcpSendFuture, TcpStream, WaitConnectedFuture,
};
pub use waker::ReactorWaker;

// Re-export smoltcp error types for convenience
pub use smoltcp::socket::tcp::{ConnectError, ListenError};

use super::DpdkDeviceWithPool;
use smoltcp::iface::{Interface, PollResult, SocketSet};
use smoltcp::phy::Device;
use smoltcp::time::Instant;
use std::cell::RefCell;
use std::rc::Rc;

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
}

impl<D: Device> ReactorInner<D> {
    /// Poll smoltcp - this is a separate method to work around borrow checker
    /// by destructuring self
    fn poll_smoltcp(&mut self, timestamp: Instant) -> PollResult {
        // By destructuring, we get separate mutable borrows
        let ReactorInner {
            device,
            iface,
            sockets,
        } = self;
        iface.poll(timestamp, device, sockets)
    }
}

/// The async reactor that drives DPDK + smoltcp
///
/// This must be polled repeatedly to make progress on network I/O.
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
            })),
        }
    }

    /// Get a handle to the reactor's inner state (for creating sockets)
    pub fn handle(&self) -> ReactorHandle {
        ReactorHandle {
            inner: self.inner.clone(),
        }
    }

    /// Poll the reactor once
    ///
    /// This polls DPDK for packets, runs smoltcp, and wakes any waiting tasks.
    /// smoltcp automatically wakes registered wakers when socket state changes.
    /// Returns true if any socket state changed.
    pub fn poll(&self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let timestamp = Instant::now();

        // Poll smoltcp - this processes packets and updates socket states
        // smoltcp will wake registered wakers when sockets can make progress
        let poll_result = inner.poll_smoltcp(timestamp);

        matches!(poll_result, PollResult::SocketStateChanged)
    }

    /// Run the reactor until the given future completes
    ///
    /// This is a single-threaded executor that polls DPDK continuously
    /// (since DPDK is poll-based) but only polls the future when:
    /// 1. A waker is triggered (socket state changed), or
    /// 2. The reactor processed packets
    ///
    /// This is more efficient than blindly polling the future every iteration.
    pub fn block_on<F: std::future::Future>(&self, mut future: F) -> F::Output {
        use std::pin::Pin;
        use std::task::{Context, Poll};

        // Create a waker that tracks when tasks are ready
        let (reactor_waker, waker) = ReactorWaker::new();
        let mut cx = Context::from_waker(&waker);

        // Pin the future on the stack
        let mut future = unsafe { Pin::new_unchecked(&mut future) };

        loop {
            // Only poll the future if a waker was triggered
            if reactor_waker.take_woken()
                && let Poll::Ready(result) = future.as_mut().poll(&mut cx)
            {
                return result;
            }

            // Poll the reactor to make progress on I/O
            // This is required because DPDK has no interrupts - we must poll for packets
            // smoltcp will wake our wakers when socket states change
            let _state_changed = self.poll();

            // Small yield to avoid burning CPU when idle
            // In production, you might want to remove this for lowest latency
            std::hint::spin_loop();
        }
    }
}

/// Handle to the reactor for creating sockets
#[derive(Clone)]
pub struct ReactorHandle {
    pub(crate) inner: Rc<RefCell<ReactorInner<DpdkDeviceWithPool>>>,
}
