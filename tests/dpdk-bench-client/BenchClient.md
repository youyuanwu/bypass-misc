# dpdk-bench-client

A high-performance HTTP benchmark client similar to `wrk` for testing DPDK-based servers.

## Overview

`dpdk-bench-client` is a load testing tool that measures throughput and latency of HTTP servers. It outputs results in JSON format, making it easy to integrate with automated benchmark pipelines.

## Client Modes

| Mode | Description | HTTP Version |
|------|-------------|--------------|
| `raw` | Raw TCP client for maximum throughput | HTTP/1.1 only |
| `hyper` | Hyper-based client with full HTTP support | HTTP/1.1 or HTTP/2 |

## Usage

```bash
# Raw TCP mode (fastest, default)
dpdk-bench-client -c 100 -d 30s http://10.0.0.4:8080/

# Hyper mode with HTTP/1.1
dpdk-bench-client --mode hyper -c 100 -d 30s http://10.0.0.4:8080/

# Hyper mode with HTTP/2
dpdk-bench-client --mode hyper --http2 -c 100 -d 30s http://10.0.0.4:8080/
```

## Command-Line Options

| Option | Default | Description |
|--------|---------|-------------|
| `<URL>` | (required) | Target URL to benchmark |
| `-c, --connections` | `10` | Number of concurrent connections |
| `-d, --duration` | `10s` | Duration of benchmark (e.g., `10s`, `1m`, `500ms`) |
| `-m, --mode` | `raw` | Client mode: `raw` or `hyper` |
| `--http2` | `false` | Use HTTP/2 (hyper mode only) |
| `--latency` | `true` | Print latency statistics |
| `--timeout` | `5000` | Request timeout in milliseconds |

## Output Format

Results are printed as JSON:

```json
{
  "url": "http://10.0.0.4:8080/",
  "connections": 100,
  "duration_secs": 30.0,
  "mode": "raw",
  "worker_threads": 4,
  "timeout_ms": 5000,
  "requests": 1500000,
  "errors": 0,
  "gb_read": 1.25,
  "requests_per_sec": 50000.0,
  "mb_per_sec": 42.5,
  "latency": {
    "p50_us": 150,
    "p75_us": 200,
    "p90_us": 350,
    "p99_us": 1200,
    "avg_us": 180,
    "max_us": 5000,
    "stdev_us": 120
  }
}
```

## Latency Tracking

Latency is recorded using HDR Histogram with:
- Range: 1µs to 60s
- 3 significant figures precision
- Percentiles: p50, p75, p90, p99
- Average, max, and standard deviation

## Dependencies

- `clap` - Command-line argument parsing
- `tokio` - Async runtime
- `hyper` / `hyper-util` - HTTP client (hyper mode)
- `h2` - HTTP/2 support
- `hdrhistogram` - Latency histogram
- `serde` / `serde_json` - JSON output
- `tracing` / `tracing-subscriber` - Logging
