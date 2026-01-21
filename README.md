# dpdk-net

[![CI](https://github.com/youyuanwu/rust-dpdk-net/actions/workflows/CI.yml/badge.svg)](https://github.com/youyuanwu/rust-dpdk-net/actions/workflows/CI.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

High-level async TCP/IP networking for Rust using [DPDK](https://www.dpdk.org/) for kernel-bypass packet I/O.

## What is this?

`dpdk-net` combines three technologies to provide high-performance networking:

- **[DPDK](https://www.dpdk.org/)** - Kernel-bypass packet I/O directly to/from the NIC
- **[smoltcp](https://github.com/smoltcp-rs/smoltcp)** - User-space TCP/IP stack
- **[tokio](https://tokio.rs/)** - Async runtime with `TcpListener` and `TcpStream` APIs

This enables building network applications (HTTP servers, proxies, etc.) that bypass the kernel network stack entirely, achieving lower latency and higher throughput.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Application Layer                            │
│   (hyper HTTP servers, TcpStream, TcpListener, custom protocols)    │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    Async Runtime Layer (tokio)                      │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      TCP/IP Stack (smoltcp)                         │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     DPDK (kernel-bypass I/O)                        │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         Hardware NIC                                │
└─────────────────────────────────────────────────────────────────────┘
```

## Features

- **Async/await support** - `TcpListener`, `TcpStream` with tokio compatibility
- **Multi-queue scaling** - RSS (Receive Side Scaling) distributes connections across CPU cores
- **CPU affinity** - Worker threads pinned to cores for optimal cache locality
- **hyper compatible** - Use with hyper for HTTP/1.1 and HTTP/2 servers

For detailed architecture documentation, see [docs/Architecture.md](docs/Architecture.md).

## Requirements

- Linux with hugepages configured
- DPDK-compatible NIC (Intel, Mellanox, etc.) or virtual device for testing
- Root privileges (for DPDK memory and device access)

## Getting Started

### 1. Install DPDK

From package manager or build from source:

```sh
# Clone this repo
cmake -S . -B build
cmake --build build --target dpdk_configure
cmake --build build --target dpdk_build --parallel
sudo cmake --build build --target dpdk_install
```

### 2. Add dependency

```toml
[dependencies]
dpdk-net = "0.1"
```

### 3. Run examples

```sh
# Build examples
cargo build --release --examples

# Run HTTP server (requires sudo and DPDK-compatible NIC)
sudo ./target/release/examples/dpdk_http_server --interface eth1
```

## Examples

- [dpdk_http_server](dpdk-net-test/examples/dpdk_http_server.rs) - HTTP server with DPDK or tokio backend
- [dpdk_tcp_server](dpdk-net-test/examples/dpdk_tcp_server.rs) - Simple TCP echo server

## Project Status

⚠️ **APIs are unstable and subject to change.**

This project is under active development. The core functionality works, but the API surface is evolving.

## References

- [rust-dpdk](https://github.com/ANLAB-KAIST/rust-dpdk) - DPDK binding generation approach
- [rpkt](https://github.com/duanjp8617/rpkt) - Rust packet processing

## License

MIT
