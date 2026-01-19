//! HTTP Server Example - DPDK or Tokio
//!
//! This example starts an HTTP server that returns an HTML page showing
//! the total number of requests received.
//!
//! Supports two modes:
//! - **dpdk**: Multi-queue DPDK + smoltcp + hyper (requires root, hardware NIC)
//! - **tokio**: Standard tokio + hyper (works anywhere)
//!
//! Usage:
//!   # DPDK mode (requires sudo)
//!   sudo -E cargo run --example dpdk_http_server -- --mode dpdk
//!
//!   # Tokio mode (no sudo needed)
//!   cargo run --example dpdk_http_server -- --mode tokio
//!
//!   # Tokio mode with custom address
//!   cargo run --example dpdk_http_server -- --mode tokio --addr 127.0.0.1:3000
//!
//! Then:
//!   curl http://localhost:8080/

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::{Parser, ValueEnum};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener as TokioTcpListener;
use tokio::signal;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

/// Global request counter shared across all connections
static REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ServerMode {
    /// DPDK + smoltcp + hyper (requires root and hardware NIC)
    Dpdk,
    /// Standard tokio + hyper
    Tokio,
}

#[derive(Parser, Debug)]
#[command(name = "http_server")]
#[command(about = "HTTP server example with DPDK or Tokio backend")]
struct Args {
    /// Server mode: dpdk or tokio
    #[arg(short, long, value_enum, default_value = "dpdk")]
    mode: ServerMode,

    /// Listen address for tokio mode (ignored in dpdk mode)
    #[arg(short, long, default_value = "0.0.0.0:8080")]
    addr: SocketAddr,

    /// Network interface for DPDK mode (ignored in tokio mode)
    #[arg(short, long, default_value = "eth1")]
    interface: String,

    /// Server port (used in DPDK mode)
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Maximum number of queues for DPDK mode
    #[arg(long, default_value = "4")]
    max_queues: usize,

    /// Listen backlog for DPDK mode (number of pending connections)
    #[arg(long, default_value = "64")]
    backlog: usize,
}

/// HTTP handler that returns an HTML page with the request count
async fn counter_handler(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let count = REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>HTTP Server</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
        }}
        .container {{
            text-align: center;
            padding: 2rem;
            background: rgba(255, 255, 255, 0.1);
            border-radius: 20px;
            backdrop-filter: blur(10px);
        }}
        h1 {{ font-size: 3rem; margin-bottom: 0.5rem; }}
        .count {{ font-size: 6rem; font-weight: bold; }}
        .label {{ font-size: 1.5rem; opacity: 0.8; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>🚀 HTTP Server</h1>
        <div class="count">{}</div>
        <div class="label">requests received</div>
    </div>
</body>
</html>"#,
        count
    );

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Full::new(Bytes::from(html)))
        .unwrap())
}

/// Run the tokio-based HTTP server
async fn run_tokio_server(addr: SocketAddr) {
    let listener = TokioTcpListener::bind(addr)
        .await
        .expect("Failed to bind address");

    info!(%addr, "Tokio HTTP server listening");

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                warn!("Received Ctrl+C, shutting down");
                break;
            }
            result = listener.accept() => {
                let (stream, peer_addr) = match result {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!(error = %e, "Accept failed");
                        continue;
                    }
                };

                tokio::spawn(async move {
                    let io = TokioIo::new(stream);

                    if let Err(e) = http1::Builder::new()
                        .serve_connection(io, service_fn(counter_handler))
                        .await
                    {
                        error!(peer = %peer_addr, error = %e, "Connection error");
                    }
                });
            }
        }
    }

    info!("Tokio HTTP server stopped");
}

/// Run the DPDK-based HTTP server
fn run_dpdk_server(interface: &str, port: u16, max_queues: usize, backlog: usize) {
    use dpdk_net_test::app::dpdk_server_runner::DpdkServerRunner;
    use dpdk_net_test::app::http_server::Http1Server;

    DpdkServerRunner::new(interface)
        .port(port)
        .max_queues(max_queues)
        .backlog(backlog)
        .tcp_buffers(16384, 16384)
        .run(|ctx| async move {
            Http1Server::new(
                ctx.listener,
                ctx.cancel,
                counter_handler,
                ctx.queue_id,
                ctx.port,
            )
            .run()
            .await
        });
}

fn main() {
    // Initialize tracing - respects RUST_LOG, defaults to info if not set
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    match args.mode {
        ServerMode::Tokio => {
            info!(mode = "tokio", "Starting HTTP server");
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");
            rt.block_on(run_tokio_server(args.addr));
        }
        ServerMode::Dpdk => {
            info!(mode = "dpdk", interface = %args.interface, port = args.port, backlog = args.backlog, "Starting HTTP server");
            run_dpdk_server(&args.interface, args.port, args.max_queues, args.backlog);
        }
    }
}
