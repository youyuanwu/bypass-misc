//! Waker implementation for the async reactor

use std::cell::Cell;
use std::rc::Rc;
use std::task::{RawWaker, RawWakerVTable, Waker};

/// A waker that tracks whether it has been woken.
///
/// This allows the reactor to know if any task is ready to make progress,
/// avoiding unnecessary future polls when nothing has changed.
pub struct ReactorWaker {
    /// Shared flag indicating a waker was triggered
    woken: Rc<Cell<bool>>,
}

impl ReactorWaker {
    /// Create a new reactor waker and its associated Waker
    pub fn new() -> (Self, Waker) {
        let woken = Rc::new(Cell::new(true)); // Start woken to poll initially
        let waker = Self::create_waker(woken.clone());
        (ReactorWaker { woken }, waker)
    }

    /// Check if the waker was triggered and reset the flag
    pub fn take_woken(&self) -> bool {
        self.woken.replace(false)
    }

    /// Create a Waker that sets the shared flag when woken
    fn create_waker(woken: Rc<Cell<bool>>) -> Waker {
        // We need to use Arc for the RawWaker since it requires Send + Sync
        // But we'll store an Rc clone inside for the actual flag
        // This is safe because we're single-threaded

        // Convert Rc to raw pointer
        let ptr = Rc::into_raw(woken) as *const ();

        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            // clone
            |ptr| {
                let woken = unsafe { Rc::from_raw(ptr as *const Cell<bool>) };
                let cloned = woken.clone();
                std::mem::forget(woken); // Don't drop the original
                RawWaker::new(Rc::into_raw(cloned) as *const (), &VTABLE)
            },
            // wake
            |ptr| {
                let woken = unsafe { Rc::from_raw(ptr as *const Cell<bool>) };
                woken.set(true);
                // Don't forget - this consumes the waker
            },
            // wake_by_ref
            |ptr| {
                let woken = unsafe { &*(ptr as *const Cell<bool>) };
                woken.set(true);
            },
            // drop
            |ptr| {
                unsafe { Rc::from_raw(ptr as *const Cell<bool>) };
                // Rc is dropped here
            },
        );

        unsafe { Waker::from_raw(RawWaker::new(ptr, &VTABLE)) }
    }
}

impl Default for ReactorWaker {
    fn default() -> Self {
        Self {
            woken: Rc::new(Cell::new(true)),
        }
    }
}
