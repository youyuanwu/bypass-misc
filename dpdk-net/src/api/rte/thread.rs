// DPDK Thread Registration API
// See: /usr/local/include/rte_thread.h

use std::marker::PhantomData;

use dpdk_net_sys::ffi;

use crate::api::check_rte_success;

/// RAII guard for DPDK thread registration.
///
/// When a non-EAL thread (e.g., a Rust `std::thread` or tokio worker) needs to
/// use DPDK APIs, it must first register with DPDK using `rte_thread_register()`.
/// This guard handles automatic registration on creation and unregistration on drop.
///
/// # Why is this needed?
///
/// DPDK uses thread-local storage for per-lcore caches (e.g., mempool caches).
/// Without registration, threads cannot efficiently allocate mbufs or use other
/// DPDK resources that depend on lcore identification.
///
/// # Example
///
/// ```no_run
/// use dpdk_net::api::rte::thread::ThreadRegistration;
/// use std::thread;
///
/// let handle = thread::spawn(|| {
///     // Register this thread with DPDK
///     let _registration = ThreadRegistration::new()
///         .expect("Failed to register thread with DPDK");
///
///     // Now this thread can use DPDK APIs (mempool, queues, etc.)
///     // ...
///
///     // Automatically unregisters when _registration is dropped
/// });
/// ```
pub struct ThreadRegistration {
    // PhantomData with *const () makes ThreadRegistration !Send and !Sync
    // This is important because the registration is tied to the current thread
    _marker: PhantomData<*const ()>,
}

impl ThreadRegistration {
    /// Register the current thread with DPDK.
    ///
    /// This must be called from the thread that needs to use DPDK APIs.
    /// The registration is automatically undone when this guard is dropped.
    ///
    /// # Errors
    ///
    /// Returns an error if registration fails (e.g., too many threads registered,
    /// or called from an already-registered EAL thread).
    pub fn new() -> crate::api::Result<Self> {
        let ret = unsafe { ffi::rte_thread_register() };
        check_rte_success(ret)?;
        Ok(Self {
            _marker: PhantomData,
        })
    }

    /// Try to register the current thread, returning None if already registered.
    ///
    /// This is useful when you're not sure if the thread is already an EAL thread
    /// or has been previously registered.
    pub fn try_new() -> Option<Self> {
        let ret = unsafe { ffi::rte_thread_register() };
        if ret == 0 {
            Some(Self {
                _marker: PhantomData,
            })
        } else {
            None
        }
    }
}

impl Drop for ThreadRegistration {
    fn drop(&mut self) {
        unsafe {
            ffi::rte_thread_unregister();
        }
    }
}
