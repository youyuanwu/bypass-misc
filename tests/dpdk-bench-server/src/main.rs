//! HTTP Benchmark Server - DPDK or Tokio
//!
//! A high-performance HTTP server for benchmarking, supporting DPDK and Tokio backends.
//!
//! Supports three modes:
//! - **dpdk**: Multi-queue DPDK + smoltcp + hyper (requires root, hardware NIC)
//! - **tokio**: Standard tokio + hyper with multi-threaded runtime (works anywhere)
//! - **tokio-local**: Thread-per-core tokio + hyper with CPU pinning (works anywhere)
//!
//! # Usage
//!
//! ```bash
//! # DPDK mode (requires sudo)
//! sudo -E dpdk-bench-server --mode dpdk
//!
//! # Tokio mode (no sudo needed)
//! dpdk-bench-server --mode tokio
//!
//! # Tokio thread-per-core mode
//! dpdk-bench-server --mode tokio-local
//!
//! # Custom address and port
//! dpdk-bench-server --mode tokio --addr 127.0.0.1:3000
//! ```
//!
//! # Testing
//!
//! ```bash
//! curl http://localhost:8080/
//! dpdk-bench-client -c 10 -d 10s http://localhost:8080/
//! ```

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::{Parser, ValueEnum};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode};
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Global request counter shared across all connections
static REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ServerMode {
    /// DPDK + smoltcp + hyper (requires root and hardware NIC)
    Dpdk,
    /// Standard tokio + hyper with multi-threaded runtime
    Tokio,
    /// Thread-per-core tokio + hyper with CPU pinning
    TokioLocal,
}

#[derive(Parser, Debug)]
#[command(name = "dpdk-bench-server")]
#[command(about = "HTTP benchmark server with DPDK or Tokio backend")]
struct Args {
    /// Server mode: dpdk, tokio, or tokio-local
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
    <title>HTTP Benchmark Server</title>
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
        <h1>🚀 HTTP Benchmark Server</h1>
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
    // Disable ANSI colors for clean log output
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_ansi(false)
        .init();

    let args = Args::parse();

    match args.mode {
        ServerMode::Tokio => {
            use dpdk_net_test::app::tokio_server::run_tokio_multi_thread_server;

            info!(mode = "tokio", addr = %args.addr, "Starting HTTP benchmark server");
            run_tokio_multi_thread_server(args.addr, counter_handler);
        }
        ServerMode::TokioLocal => {
            use dpdk_net_test::app::tokio_server::run_tokio_thread_per_core_server;

            info!(mode = "tokio-local", addr = %args.addr, "Starting HTTP benchmark server");
            run_tokio_thread_per_core_server(args.addr, counter_handler);
        }
        ServerMode::Dpdk => {
            info!(
                mode = "dpdk",
                interface = %args.interface,
                port = args.port,
                max_queues = args.max_queues,
                backlog = args.backlog,
                "Starting HTTP benchmark server"
            );
            run_dpdk_server(&args.interface, args.port, args.max_queues, args.backlog);
        }
    }
}
