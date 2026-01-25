# dpdk-bench-server

A high-performance HTTP benchmark server supporting multiple networking backends for comparative performance testing.

## Overview

`dpdk-bench-server` is a simple HTTP server designed for benchmarking network I/O performance. It serves an HTML page displaying a global request counter, making it easy to verify the server is handling requests correctly while measuring throughput and latency.

## Server Modes

The server supports three operating modes:

| Mode | Description | Requirements |
|------|-------------|--------------|
| `dpdk` | Multi-queue DPDK + smoltcp + hyper | Root privileges, hardware NIC with DPDK support |
| `tokio` | Standard tokio + hyper with multi-threaded runtime | None (works anywhere) |
| `tokio-local` | Thread-per-core tokio + hyper with CPU pinning | None (works anywhere) |

## Usage

```bash
# DPDK mode (requires sudo)
sudo -E dpdk-bench-server --mode dpdk --interface eth1 --port 8080

# Tokio multi-threaded mode
dpdk-bench-server --mode tokio --addr 0.0.0.0:8080

# Tokio thread-per-core mode
dpdk-bench-server --mode tokio-local --addr 0.0.0.0:8080
```

## Command-Line Options

| Option | Default | Description |
|--------|---------|-------------|
| `-m, --mode` | `dpdk` | Server mode: `dpdk`, `tokio`, or `tokio-local` |
| `-a, --addr` | `0.0.0.0:8080` | Listen address (tokio modes only) |
| `-i, --interface` | `eth1` | Network interface (DPDK mode only) |
| `-p, --port` | `8080` | Server port (DPDK mode only) |
| `--max-queues` | `4` | Maximum number of queues (DPDK mode only) |
| `--backlog` | `64` | Listen backlog for pending connections (DPDK mode only) |

## Testing

```bash
# Simple connectivity test
curl http://localhost:8080/

# Benchmark with dpdk-bench-client
dpdk-bench-client -c 10 -d 10s http://localhost:8080/
```

## Dependencies

- `clap` - Command-line argument parsing
- `tokio` - Async runtime for tokio modes
- `hyper` - HTTP implementation
- `dpdk-net-test` - Server implementations (DPDK and tokio runners)
- `tracing` / `tracing-subscriber` - Logging
