//! Waker implementation for the async reactor

use std::task::Waker;

/// A no-op waker for the reactor's block_on executor
///
/// Since DPDK is poll-based, we don't need to wake anything -
/// the reactor is always polling. This waker just exists to
/// satisfy the Future API.
pub struct ReactorWaker;

impl ReactorWaker {
    /// Create a new no-op waker
    ///
    /// Since the reactor busy-polls, waking does nothing.
    pub fn create() -> Waker {
        Waker::noop().clone()
    }
}

impl Default for ReactorWaker {
    fn default() -> Self {
        Self
    }
}
