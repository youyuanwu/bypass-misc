//! Async TCP socket implementation

use super::{ReactorHandle, ReactorInner, SocketWakers};
use crate::tcp::DpdkDeviceWithPool;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp::{self, ConnectError, ListenError, RecvError, SendError, State};
use smoltcp::wire::IpAddress;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

/// An async TCP socket
///
/// This wraps a smoltcp TCP socket and provides async send/recv operations.
pub struct AsyncTcpSocket {
    pub(crate) handle: SocketHandle,
    pub(crate) reactor: Rc<RefCell<ReactorInner<DpdkDeviceWithPool>>>,
}

impl AsyncTcpSocket {
    /// Create a new TCP socket for listening
    ///
    /// Returns an error if the socket is not in a valid state to listen
    /// (e.g., already connected or listening).
    pub fn listen(
        handle: &ReactorHandle,
        port: u16,
        rx_buffer_size: usize,
        tx_buffer_size: usize,
    ) -> Result<Self, ListenError> {
        let mut inner = handle.inner.borrow_mut();

        let rx_buffer = tcp::SocketBuffer::new(vec![0; rx_buffer_size]);
        let tx_buffer = tcp::SocketBuffer::new(vec![0; tx_buffer_size]);
        let mut socket = tcp::Socket::new(rx_buffer, tx_buffer);
        socket.listen(port)?;

        let socket_handle = inner.sockets.add(socket);
        inner.wakers.insert(socket_handle, SocketWakers::default());

        Ok(AsyncTcpSocket {
            handle: socket_handle,
            reactor: handle.inner.clone(),
        })
    }

    /// Create a new TCP socket and connect to a remote endpoint
    ///
    /// Returns an error if the connection cannot be initiated (e.g., invalid
    /// state, unspecified local/remote addresses, or port already in use).
    pub fn connect(
        handle: &ReactorHandle,
        remote_addr: IpAddress,
        remote_port: u16,
        local_port: u16,
        rx_buffer_size: usize,
        tx_buffer_size: usize,
    ) -> Result<Self, ConnectError> {
        let mut inner = handle.inner.borrow_mut();

        let rx_buffer = tcp::SocketBuffer::new(vec![0; rx_buffer_size]);
        let tx_buffer = tcp::SocketBuffer::new(vec![0; tx_buffer_size]);
        let mut socket = tcp::Socket::new(rx_buffer, tx_buffer);

        // Connect before adding to socket set
        socket.connect(
            inner.iface.context(),
            (remote_addr, remote_port),
            local_port,
        )?;

        let socket_handle = inner.sockets.add(socket);
        inner.wakers.insert(socket_handle, SocketWakers::default());

        Ok(AsyncTcpSocket {
            handle: socket_handle,
            reactor: handle.inner.clone(),
        })
    }

    /// Get the socket handle
    pub fn handle(&self) -> SocketHandle {
        self.handle
    }

    /// Check if the socket is connected (in Established state)
    pub fn is_connected(&self) -> bool {
        let inner = self.reactor.borrow();
        let socket = inner.sockets.get::<tcp::Socket>(self.handle);
        socket.state() == State::Established
    }

    /// Check if the socket is active (exchanging data)
    pub fn is_active(&self) -> bool {
        let inner = self.reactor.borrow();
        let socket = inner.sockets.get::<tcp::Socket>(self.handle);
        socket.is_active()
    }

    /// Get the current socket state
    pub fn state(&self) -> State {
        let inner = self.reactor.borrow();
        let socket = inner.sockets.get::<tcp::Socket>(self.handle);
        socket.state()
    }

    /// Send data asynchronously
    ///
    /// Returns the number of bytes sent when the operation completes.
    pub fn send<'a>(&'a self, data: &'a [u8]) -> TcpSendFuture<'a> {
        TcpSendFuture {
            socket: self,
            data,
            offset: 0,
        }
    }

    /// Receive data asynchronously
    ///
    /// Returns the number of bytes received when the operation completes.
    /// Returns 0 if the connection was closed gracefully.
    pub fn recv<'a>(&'a self, buf: &'a mut [u8]) -> TcpRecvFuture<'a> {
        TcpRecvFuture { socket: self, buf }
    }

    /// Wait for the socket to become connected
    pub fn wait_connected(&self) -> WaitConnectedFuture<'_> {
        WaitConnectedFuture { socket: self }
    }

    /// Close the socket
    pub fn close(&self) {
        let mut inner = self.reactor.borrow_mut();
        let socket = inner.sockets.get_mut::<tcp::Socket>(self.handle);
        socket.close();
    }

    /// Abort the connection immediately
    pub fn abort(&self) {
        let mut inner = self.reactor.borrow_mut();
        let socket = inner.sockets.get_mut::<tcp::Socket>(self.handle);
        socket.abort();
    }
}

impl Drop for AsyncTcpSocket {
    fn drop(&mut self) {
        let mut inner = self.reactor.borrow_mut();

        // Abort if not already closed to notify peer
        let socket = inner.sockets.get_mut::<tcp::Socket>(self.handle);
        if socket.state() != State::Closed {
            socket.abort();
        }

        // Remove from socket set and wakers
        inner.sockets.remove(self.handle);
        inner.wakers.remove(&self.handle);
    }
}

/// Future for sending data on a TCP socket
pub struct TcpSendFuture<'a> {
    socket: &'a AsyncTcpSocket,
    data: &'a [u8],
    offset: usize,
}

impl<'a> Future for TcpSendFuture<'a> {
    type Output = Result<usize, SendError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.socket.reactor.borrow_mut();

        // Register the waker
        if let Some(wakers) = inner.wakers.get_mut(&self.socket.handle) {
            wakers.send_waker = Some(cx.waker().clone());
        }

        let socket = inner.sockets.get_mut::<tcp::Socket>(self.socket.handle);

        // Check if we can send
        if !socket.may_send() {
            return Poll::Ready(Err(SendError::InvalidState));
        }

        if !socket.can_send() {
            // Buffer is full, wait
            return Poll::Pending;
        }

        // Try to send remaining data
        let remaining = &self.data[self.offset..];
        match socket.send_slice(remaining) {
            Ok(sent) => {
                self.offset += sent;
                if self.offset >= self.data.len() {
                    // All data sent
                    Poll::Ready(Ok(self.data.len()))
                } else {
                    // More data to send, continue waiting
                    Poll::Pending
                }
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

/// Future for receiving data from a TCP socket
pub struct TcpRecvFuture<'a> {
    socket: &'a AsyncTcpSocket,
    buf: &'a mut [u8],
}

impl<'a> Future for TcpRecvFuture<'a> {
    type Output = Result<usize, RecvError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut inner = this.socket.reactor.borrow_mut();

        // Register the waker
        if let Some(wakers) = inner.wakers.get_mut(&this.socket.handle) {
            wakers.recv_waker = Some(cx.waker().clone());
        }

        let socket = inner.sockets.get_mut::<tcp::Socket>(this.socket.handle);

        // Check if we can receive
        if !socket.may_recv() {
            // Check if this is a graceful close or an error
            if socket.state() == State::CloseWait
                || socket.state() == State::Closed
                || socket.state() == State::TimeWait
            {
                return Poll::Ready(Ok(0)); // EOF
            }
            return Poll::Ready(Err(RecvError::InvalidState));
        }

        if !socket.can_recv() {
            // No data available, wait
            return Poll::Pending;
        }

        // Try to receive data
        match socket.recv_slice(this.buf) {
            Ok(len) => Poll::Ready(Ok(len)),
            Err(RecvError::Finished) => Poll::Ready(Ok(0)), // EOF
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

/// Future for waiting until a socket is connected
pub struct WaitConnectedFuture<'a> {
    socket: &'a AsyncTcpSocket,
}

impl<'a> Future for WaitConnectedFuture<'a> {
    type Output = Result<(), ()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.socket.reactor.borrow_mut();

        // Register the waker for send events (connection completion triggers this)
        if let Some(wakers) = inner.wakers.get_mut(&self.socket.handle) {
            wakers.send_waker = Some(cx.waker().clone());
        }

        let socket = inner.sockets.get::<tcp::Socket>(self.socket.handle);

        match socket.state() {
            State::Established => Poll::Ready(Ok(())),
            State::Closed | State::TimeWait => Poll::Ready(Err(())),
            // Client states: waiting for handshake to complete
            State::SynSent | State::SynReceived => Poll::Pending,
            // Server states: waiting for incoming connection
            State::Listen => Poll::Pending,
            _ => Poll::Pending,
        }
    }
}
