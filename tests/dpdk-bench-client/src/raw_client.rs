//! Raw TCP-based HTTP/1.1 client for maximum throughput
//!
//! This implementation uses tokio TcpStream directly with minimal parsing
//! to achieve the highest possible request rate, similar to wrk.

use crate::stats::{BenchStats, LocalStats};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Parsed URL components
struct ParsedUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let url = url
        .strip_prefix("http://")
        .ok_or("Only http:// URLs supported")?;

    let (host_port, path) = if let Some(idx) = url.find('/') {
        (&url[..idx], &url[idx..])
    } else {
        (url, "/")
    };

    let (host, port) = if let Some(idx) = host_port.find(':') {
        let host = &host_port[..idx];
        let port: u16 = host_port[idx + 1..].parse().map_err(|_| "Invalid port")?;
        (host.to_string(), port)
    } else {
        (host_port.to_string(), 80)
    };

    Ok(ParsedUrl {
        host,
        port,
        path: path.to_string(),
    })
}

/// Run the benchmark using raw TCP connections
pub async fn run_benchmark(
    url: &str,
    connections: usize,
    duration: Duration,
    timeout: Duration,
    stats: Arc<BenchStats>,
) {
    let parsed = parse_url(url).expect("Invalid URL");
    let addr = format!("{}:{}", parsed.host, parsed.port);

    // Pre-build the HTTP request bytes
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: keep-alive\r\n\r\n",
        parsed.path, parsed.host
    );
    let request_bytes: Arc<[u8]> = request.into_bytes().into();

    let end_time = Instant::now() + duration;

    let mut handles = Vec::with_capacity(connections);
    for _ in 0..connections {
        let addr = addr.clone();
        let request_bytes = request_bytes.clone();
        let stats = stats.clone();

        let handle = tokio::spawn(async move {
            run_connection(addr, request_bytes, stats, end_time, timeout).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }
}

async fn run_connection(
    addr: String,
    request_bytes: Arc<[u8]>,
    stats: Arc<BenchStats>,
    end_time: Instant,
    timeout: Duration,
) {
    stats.active_connections.fetch_add(1, Ordering::Relaxed);

    let mut local = LocalStats::new();
    let mut stream: Option<TcpStream> = None;
    // Reusable buffer - large enough for headers + body in most cases
    let mut buf = vec![0u8; 16384];

    while Instant::now() < end_time {
        // Ensure we have a connection
        if stream.is_none() {
            match tokio::time::timeout(timeout, TcpStream::connect(&addr)).await {
                Ok(Ok(s)) => {
                    let _ = s.set_nodelay(true);
                    stream = Some(s);
                }
                Ok(Err(e)) => {
                    local.record_error();
                    stats.record_error_sample(format!("connect: {}", e));
                    continue;
                }
                Err(_) => {
                    local.record_error();
                    stats.record_error_sample("connect timeout".to_string());
                    continue;
                }
            }
        }

        let s = stream.as_mut().unwrap();
        let start = Instant::now();

        let result = tokio::time::timeout(timeout, async {
            // Send request
            s.write_all(&request_bytes).await?;

            // Read response into buffer
            let mut total_read = 0usize;
            let mut headers_end = None;
            let mut content_length: Option<usize> = None;

            // Read until we have complete headers
            loop {
                let n = s.read(&mut buf[total_read..]).await?;
                if n == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "connection closed",
                    ));
                }
                total_read += n;

                // Look for end of headers
                if headers_end.is_none()
                    && let Some(pos) = find_header_end(&buf[..total_read])
                {
                    headers_end = Some(pos);
                    // Parse Content-Length from headers
                    content_length = parse_content_length(&buf[..pos]);
                }

                if let Some(hdr_end) = headers_end {
                    let body_received = total_read - hdr_end;
                    if let Some(cl) = content_length {
                        if body_received >= cl {
                            // Complete response received
                            break;
                        }
                    } else {
                        // No Content-Length, assume response is complete after first read
                        // This works for simple responses without body
                        break;
                    }
                }

                // Need more buffer space
                if total_read >= buf.len() {
                    buf.resize(buf.len() * 2, 0);
                }
            }

            // If we have Content-Length and need more body data, keep reading
            if let (Some(hdr_end), Some(cl)) = (headers_end, content_length) {
                let mut body_received = total_read - hdr_end;
                while body_received < cl {
                    let remaining = cl - body_received;
                    // Read directly and discard - we don't need the data
                    let to_read = remaining.min(buf.len());
                    let n = s.read(&mut buf[..to_read]).await?;
                    if n == 0 {
                        break;
                    }
                    body_received += n;
                    total_read += n;
                }
            }

            Ok::<usize, std::io::Error>(total_read)
        })
        .await;

        let latency_us = start.elapsed().as_micros() as u64;

        match result {
            Ok(Ok(bytes)) => {
                local.record_success(latency_us, bytes);
            }
            Ok(Err(e)) => {
                local.record_error();
                stats.record_error_sample(format!("io: {}", e));
                stream = None; // Reconnect on error
            }
            Err(_) => {
                local.record_error();
                stats.record_error_sample("timeout".to_string());
                stream = None; // Reconnect on timeout
            }
        }
    }

    local.merge_into(&stats);
    stats.active_connections.fetch_sub(1, Ordering::Relaxed);
}

/// Find the end of HTTP headers (position after \r\n\r\n)
#[inline]
fn find_header_end(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
    }
    None
}

/// Parse Content-Length from headers
#[inline]
fn parse_content_length(headers: &[u8]) -> Option<usize> {
    // Simple case-insensitive search for "content-length:"
    let headers_str = std::str::from_utf8(headers).ok()?;
    for line in headers_str.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            return value.trim().parse().ok();
        }
    }
    None
}
