//! Hyper-based HTTP client implementation
//!
//! Uses hyper_util for connection pooling and HTTP/2 support.

use crate::stats::{BenchStats, LocalStats};
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub type HttpClient = Client<hyper_util::client::legacy::connect::HttpConnector, Empty<Bytes>>;

/// Build a hyper HTTP client
pub fn build_client(http2: bool) -> HttpClient {
    if http2 {
        Client::builder(TokioExecutor::new())
            .http2_only(true)
            .build_http::<Empty<Bytes>>()
    } else {
        Client::builder(TokioExecutor::new())
            .pool_idle_timeout(Duration::from_secs(30))
            .build_http::<Empty<Bytes>>()
    }
}

/// Run the benchmark using hyper client
pub async fn run_benchmark(
    url: &str,
    connections: usize,
    duration: Duration,
    timeout: Duration,
    http2: bool,
    stats: Arc<BenchStats>,
) {
    let uri: hyper::Uri = url.parse().expect("Invalid URL");
    let client = build_client(http2);
    let end_time = Instant::now() + duration;

    let mut handles = Vec::with_capacity(connections);
    for _ in 0..connections {
        let client = client.clone();
        let uri = uri.clone();
        let stats = stats.clone();

        let handle = tokio::spawn(async move {
            run_connection(client, uri, stats, end_time, timeout).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }
}

async fn run_connection(
    client: HttpClient,
    uri: hyper::Uri,
    stats: Arc<BenchStats>,
    end_time: Instant,
    timeout: Duration,
) {
    stats.active_connections.fetch_add(1, Ordering::Relaxed);

    let mut local = LocalStats::new();

    while Instant::now() < end_time {
        let start = Instant::now();

        let result = tokio::time::timeout(timeout, async {
            let req = hyper::Request::builder()
                .method(hyper::Method::GET)
                .uri(&uri)
                .body(Empty::<Bytes>::new())
                .unwrap();

            let response = client.request(req).await.map_err(|e| {
                let mut msg = e.to_string();
                let mut source = e.source();
                while let Some(cause) = source {
                    msg.push_str(" -> ");
                    msg.push_str(&cause.to_string());
                    source = cause.source();
                }
                msg
            })?;

            let mut body = response.into_body();
            let mut total_bytes = 0usize;
            while let Some(frame) = body.frame().await {
                if let Ok(frame) = frame
                    && let Some(data) = frame.data_ref()
                {
                    total_bytes += data.len();
                }
            }

            Ok::<usize, String>(total_bytes)
        })
        .await;

        let latency_us = start.elapsed().as_micros() as u64;

        match result {
            Ok(Ok(bytes)) => {
                local.record_success(latency_us, bytes);
            }
            Ok(Err(e)) => {
                local.record_error();
                stats.record_error_sample(e);
            }
            Err(_) => {
                local.record_error();
                stats.record_error_sample("timeout".to_string());
            }
        }
    }

    local.merge_into(&stats);
    stats.active_connections.fetch_sub(1, Ordering::Relaxed);
}
