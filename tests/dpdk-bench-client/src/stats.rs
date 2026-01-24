//! Shared statistics collection for benchmark clients

use hdrhistogram::Histogram;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::sync::Mutex;

/// Statistics collected during the benchmark
pub struct BenchStats {
    /// Total number of successful requests
    pub requests: AtomicU64,
    /// Total number of failed requests
    pub errors: AtomicU64,
    /// Total bytes read
    pub bytes_read: AtomicU64,
    /// Latency histogram (in microseconds)
    pub latency_histogram: Mutex<Histogram<u64>>,
    /// Number of active connections
    pub active_connections: AtomicUsize,
    /// First N error messages for debugging
    pub error_samples: Mutex<Vec<String>>,
    /// Max error samples to collect
    pub max_error_samples: usize,
}

impl BenchStats {
    pub fn new() -> Self {
        Self {
            requests: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            // Histogram for latencies from 1µs to 60s with 3 significant figures
            latency_histogram: Mutex::new(
                Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).unwrap(),
            ),
            active_connections: AtomicUsize::new(0),
            error_samples: Mutex::new(Vec::new()),
            max_error_samples: 5,
        }
    }

    pub fn record_error_sample(&self, msg: String) {
        if let Ok(mut samples) = self.error_samples.try_lock()
            && samples.len() < self.max_error_samples
        {
            samples.push(msg);
        }
    }

    pub fn get_requests(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    pub fn get_errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    pub fn get_bytes(&self) -> u64 {
        self.bytes_read.load(Ordering::Relaxed)
    }
}

impl Default for BenchStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Local stats accumulated per connection worker to avoid contention
pub struct LocalStats {
    pub histogram: Histogram<u64>,
    pub requests: u64,
    pub bytes: u64,
    pub errors: u64,
}

impl LocalStats {
    pub fn new() -> Self {
        Self {
            histogram: Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).unwrap(),
            requests: 0,
            bytes: 0,
            errors: 0,
        }
    }

    pub fn record_success(&mut self, latency_us: u64, bytes: usize) {
        self.requests += 1;
        self.bytes += bytes as u64;
        let _ = self.histogram.record(latency_us);
    }

    pub fn record_error(&mut self) {
        self.errors += 1;
    }

    pub fn merge_into(self, stats: &Arc<BenchStats>) {
        stats.requests.fetch_add(self.requests, Ordering::Relaxed);
        stats.bytes_read.fetch_add(self.bytes, Ordering::Relaxed);
        stats.errors.fetch_add(self.errors, Ordering::Relaxed);

        if let Ok(mut hist) = stats.latency_histogram.try_lock() {
            let _ = hist.add(&self.histogram);
        }
    }
}

impl Default for LocalStats {
    fn default() -> Self {
        Self::new()
    }
}
