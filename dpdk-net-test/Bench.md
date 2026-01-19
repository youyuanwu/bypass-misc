# Bench HTTP Server

## Install wrk

```sh
sudo apt install wrk
```

## Run Benchmark

```sh
wrk -t4 -c100 -d30s http://localhost:8080/
```

### Options

| Flag | Description |
|------|-------------|
| `-t4` | Use 4 threads |
| `-c100` | Keep 100 HTTP connections open |
| `-d30s` | Run benchmark for 30 seconds |

### What wrk does

`wrk` is a high-performance HTTP benchmarking tool that:

- Opens multiple concurrent connections to the server
- Sends as many requests as possible during the test duration
- Measures throughput (requests/sec) and latency statistics
- Reports min/max/avg/stdev latency and percentiles

### Example Output

```
Running 30s test @ http://localhost:8080/
  4 threads and 100 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency     1.23ms  234.56us   5.67ms   89.12%
    Req/Sec    20.12k     1.23k   25.00k    75.00%
  2400000 requests in 30.00s, 300.00MB read
Requests/sec:  80000.00
Transfer/sec:     10.00MB
```

### HTTPS Testing

For HTTPS endpoints:

```sh
wrk -t4 -c100 -d30s https://localhost:8443/
```

### Run http server
```sh
sudo target/release/examples/dpdk_http_server
target/release/examples/dpdk_http_server --mode tokio

# run tests
DPDK_IP=""
TOKIO_IP=""
N_THREAD=8
N_CONN=100
wrk "-t${N_THREAD}" "-c${N_CONN}" -d30s http://${DPDK_IP}:8080/
wrk "-t${N_THREAD}" "-c${N_CONN}" -d30s http://${TOKIO_IP}:8080/
```

TODO: Same zone result.

```sh
seq 5 | xargs -I{} timeout 1s curl http://${DPDK_IP}:8080/

for _ in {1..5}; do timeout 1s curl http://${DPDK_IP}:8080/ || true; done
```