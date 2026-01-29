//! Tokio-based HTTP server implementation.

use std::net::SocketAddr;

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener as TokioTcpListener;
use tokio::signal;
use tracing::{error, info, warn};

/// Wrap a handler that takes `Request<Bytes>` to work with hyper's `Request<Incoming>`.
///
/// This adapter collects the streaming body into `Bytes` before calling the handler,
/// allowing handlers to be written with non-streaming body types.
#[allow(clippy::type_complexity)]
pub fn with_collected_body<F, Fut>(
    handler: F,
) -> impl Fn(
    Request<Incoming>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Response<Full<Bytes>>, hyper::Error>> + Send>,
> + Clone
+ Send
+ Sync
where
    F: Fn(Request<Bytes>) -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Response<Full<Bytes>>, hyper::Error>> + Send + 'static,
{
    move |req: Request<Incoming>| {
        let handler = handler.clone();
        Box::pin(async move {
            // Split request into parts and body
            let (parts, body) = req.into_parts();
            // Collect the body
            let body_bytes = body.collect().await?.to_bytes();
            // Reconstruct with Bytes body
            let req = Request::from_parts(parts, body_bytes);
            handler(req).await
        })
    }
}

/// Run the tokio-based HTTP server with a multi-threaded runtime.
///
/// This function creates a multi-threaded tokio runtime and starts a standard
/// tokio + hyper HTTP server that accepts connections and processes them using
/// the provided handler.
///
/// # Arguments
/// * `addr` - The socket address to bind to
/// * `handler` - An async function that handles HTTP requests with collected body
pub fn run_tokio_multi_thread_server<F, Fut>(addr: SocketAddr, handler: F)
where
    F: Fn(Request<Bytes>) -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Response<Full<Bytes>>, hyper::Error>> + Send + 'static,
{
    let wrapped_handler = with_collected_body(handler);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    rt.block_on(async move {
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

                    let handler = wrapped_handler.clone();
                    tokio::spawn(async move {
                        let io = TokioIo::new(stream);

                        if let Err(e) = http1::Builder::new()
                            .serve_connection(io, service_fn(handler))
                            .await
                        {
                            tracing::debug!(peer = %peer_addr, error = %e, "Connection error");
                        }
                    });
                }
            }
        }

        info!("Tokio HTTP server stopped");
    });
}

/// Run the tokio-based HTTP server with a thread-per-core runtime.
///
/// This function spawns one thread per CPU core, each with its own single-threaded
/// tokio runtime. Each thread binds to a specific core and shares the same listening
/// socket using SO_REUSEPORT for load balancing across cores.
///
/// The main thread is also utilized as a worker (core 0), with additional worker
/// threads spawned for the remaining cores.
///
/// # Arguments
/// * `addr` - The socket address to bind to
/// * `handler` - An async function that handles HTTP requests with collected body
pub fn run_tokio_thread_per_core_server<F, Fut>(addr: SocketAddr, handler: F)
where
    F: Fn(Request<Bytes>) -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Response<Full<Bytes>>, hyper::Error>> + Send + 'static,
{
    use std::thread;
    use tokio_util::sync::CancellationToken;

    let wrapped_handler = with_collected_body(handler);

    let num_cores = thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1);

    info!(cores = num_cores, %addr, "Starting thread-per-core HTTP server");

    let cancel_token = CancellationToken::new();
    let mut handles = Vec::with_capacity(num_cores.saturating_sub(1));

    // Spawn worker threads for cores 1..num_cores (main thread handles core 0)
    for core_id in 1..num_cores {
        let handler = wrapped_handler.clone();
        let cancel_token = cancel_token.clone();

        let handle = thread::Builder::new()
            .name(format!("tokio-core-{}", core_id))
            .spawn(move || {
                run_worker(core_id, addr, handler, cancel_token);
            })
            .expect("Failed to spawn worker thread");

        handles.push(handle);
    }

    // Run core 0 on the main thread, with signal handler
    {
        // Pin main thread to core 0
        if let Err(e) = dpdk_net::api::rte::thread::set_cpu_affinity(0) {
            warn!(core = 0, error = %e, "Failed to set CPU affinity");
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        let cancel_token_for_signal = cancel_token.clone();
        rt.block_on(async move {
            // Spawn signal handler task
            tokio::spawn(async move {
                if signal::ctrl_c().await.is_ok() {
                    warn!("Received Ctrl+C, shutting down");
                    cancel_token_for_signal.cancel();
                }
            });

            // Run the accept loop for core 0
            run_accept_loop(0, addr, wrapped_handler, cancel_token).await;
        });
    }

    // Wait for all worker threads to finish
    for handle in handles {
        let _ = handle.join();
    }

    info!("Thread-per-core HTTP server stopped");
}

/// Run a worker on the specified core.
fn run_worker<F>(
    core_id: usize,
    addr: SocketAddr,
    handler: F,
    cancel_token: tokio_util::sync::CancellationToken,
) where
    F: Fn(
            Request<Incoming>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Response<Full<Bytes>>, hyper::Error>>
                    + Send,
            >,
        > + Clone
        + Send
        + 'static,
{
    // Pin thread to core for better cache locality
    if let Err(e) = dpdk_net::api::rte::thread::set_cpu_affinity(core_id) {
        warn!(core = core_id, error = %e, "Failed to set CPU affinity");
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    rt.block_on(run_accept_loop(core_id, addr, handler, cancel_token));
}

/// Run the accept loop for a single core.
async fn run_accept_loop<F>(
    core_id: usize,
    addr: SocketAddr,
    handler: F,
    cancel_token: tokio_util::sync::CancellationToken,
) where
    F: Fn(
            Request<Incoming>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Response<Full<Bytes>>, hyper::Error>>
                    + Send,
            >,
        > + Clone
        + Send
        + 'static,
{
    // Use socket2 to create a socket with SO_REUSEPORT
    let socket = socket2::Socket::new(
        if addr.is_ipv4() {
            socket2::Domain::IPV4
        } else {
            socket2::Domain::IPV6
        },
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .expect("Failed to create socket");

    socket
        .set_reuse_port(true)
        .expect("Failed to set SO_REUSEPORT");
    socket
        .set_reuse_address(true)
        .expect("Failed to set SO_REUSEADDR");
    socket
        .set_nonblocking(true)
        .expect("Failed to set non-blocking");
    socket.bind(&addr.into()).expect("Failed to bind address");
    socket.listen(1024).expect("Failed to listen");

    let listener = TokioTcpListener::from_std(socket.into())
        .expect("Failed to create TcpListener from socket");

    info!(core = core_id, "Worker listening");

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                break;
            }
            result = listener.accept() => {
                let (stream, peer_addr) = match result {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!(core = core_id, error = %e, "Accept failed");
                        continue;
                    }
                };

                let handler = handler.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);

                    if let Err(e) = http1::Builder::new()
                        .serve_connection(io, service_fn(handler))
                        .await
                    {
                        error!(peer = %peer_addr, error = %e, "Connection error");
                    }
                });
            }
        }
    }

    info!(core = core_id, "Worker stopped");
}
