//! Async networking with DPDK and smoltcp
//!
//! This module provides async/await support for TCP connections using DPDK
//! as the underlying packet I/O layer and smoltcp for the TCP/IP stack.
//!
//! # Architecture
//!
//! Unlike interrupt-driven async (e.g., embassy, tokio with epoll), DPDK is poll-based.
//! This means the async model here is cooperative:
//!
//! 1. A reactor polls DPDK + smoltcp in a loop
//! 2. smoltcp wakes registered wakers when socket state changes
//! 3. Async tasks yield when they can't make progress
//! 4. The executor runs woken tasks
//!
//! # Example
//!
//! ```no_run
//! use dpdk_net::async_net::{AsyncTcpSocket, Reactor};
//! use dpdk_net::tcp::DpdkDeviceWithPool;
//! use smoltcp::iface::{Config, Interface};
//! use smoltcp::time::Instant;
//! use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};
//!
//! fn example(device: DpdkDeviceWithPool, iface: Interface) {
//!     // Create reactor with DPDK device and smoltcp interface
//!     let reactor = Reactor::new(device, iface);
//!     let handle = reactor.handle();
//!
//!     // Create a listening socket
//!     let server = AsyncTcpSocket::listen(&handle, 8080, 4096, 4096)
//!         .expect("listen failed");
//!
//!     // Run async code with the reactor
//!     reactor.block_on(async {
//!         // Wait for connection
//!         server.wait_connected().await.expect("accept failed");
//!
//!         // Echo received data
//!         let mut buf = [0u8; 1024];
//!         let n = server.recv(&mut buf).await.expect("recv failed");
//!         server.send(&buf[..n]).await.expect("send failed");
//!     });
//! }
//! ```

mod socket;
mod waker;

pub use socket::{AsyncTcpSocket, TcpRecvFuture, TcpSendFuture};
pub use waker::ReactorWaker;

// Re-export smoltcp error types for convenience
pub use smoltcp::socket::tcp::{ConnectError, ListenError};

use crate::tcp::DpdkDeviceWithPool;
use smoltcp::iface::{Interface, PollResult, SocketHandle, SocketSet};
use smoltcp::phy::Device;
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::task::Waker;

/// Shared state for the async reactor
///
/// This holds all the smoltcp state and provides interior mutability
/// so that futures can access it.
pub struct ReactorInner<D: Device> {
    pub device: D,
    pub iface: Interface,
    pub sockets: SocketSet<'static>,
    /// Wakers waiting for socket events
    pub wakers: HashMap<SocketHandle, SocketWakers>,
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
            ..
        } = self;
        iface.poll(timestamp, device, sockets)
    }
}

/// Wakers registered for a socket
#[derive(Default)]
pub struct SocketWakers {
    pub recv_waker: Option<Waker>,
    pub send_waker: Option<Waker>,
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
                wakers: HashMap::new(),
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
    /// Returns true if any work was done.
    pub fn poll(&self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let timestamp = Instant::now();

        // Poll smoltcp - this processes packets and updates socket states
        let poll_result = inner.poll_smoltcp(timestamp);

        // Wake any tasks whose sockets can now make progress
        for (handle, wakers) in &inner.wakers {
            let socket = inner.sockets.get::<tcp::Socket>(*handle);

            // Wake recv waker if we can receive
            if (socket.can_recv() || !socket.may_recv())
                && let Some(waker) = &wakers.recv_waker
            {
                waker.wake_by_ref();
            }

            // Wake send waker if we can send
            if (socket.can_send() || !socket.may_send())
                && let Some(waker) = &wakers.send_waker
            {
                waker.wake_by_ref();
            }
        }

        // Check if any socket state changed
        matches!(poll_result, PollResult::SocketStateChanged)
    }

    /// Run the reactor until the given future completes
    ///
    /// This is a simple single-threaded executor that busy-polls.
    pub fn block_on<F: std::future::Future>(&self, mut future: F) -> F::Output {
        use std::pin::Pin;
        use std::task::{Context, Poll};

        let waker = ReactorWaker::create();
        let mut cx = Context::from_waker(&waker);

        // Pin the future on the stack
        let mut future = unsafe { Pin::new_unchecked(&mut future) };

        loop {
            // Poll the future
            if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
                return result;
            }

            // Poll the reactor to make progress on I/O
            self.poll();

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
