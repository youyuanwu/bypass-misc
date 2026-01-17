# DPDK TCP Server Example

This example demonstrates a TCP echo server running on DPDK with smoltcp.

## What it does

- Binds to eth1 (10.0.0.5) using DPDK
- Starts a TCP server listening on port 8080
- Echoes back any data received from clients
- Uses smoltcp for the TCP/IP stack

## Prerequisites

- Hugepages configured (automatically done by the example)
- eth1 interface available and configured
- Root/sudo access for DPDK

## Running the server

```bash
cargo run --example dpdk_tcp_server
```

## Testing from a client

From another machine on the same network (10.0.0.0/24):

```bash
# Using netcat
nc 10.0.0.5 8080

# Using telnet
telnet 10.0.0.5 8080

# Using Python
python3 -c "import socket; s=socket.socket(); s.connect(('10.0.0.5', 8080)); s.send(b'Hello'); print(s.recv(1024)); s.close()"
```

Type any message and it will be echoed back.

## Test through public ip.
Primary public ip is used by ssh.
So the vm has 2 public ip. The second one is bound to eth1 where dpdk operates on.

## Why not test from the same machine?

When DPDK binds to eth1, that NIC is removed from the kernel's control. If you try to connect from eth0 on the same machine:
- Both eth0 (10.0.0.4) and eth1 (10.0.0.5) are on the same subnet
- The kernel on eth0 can't ARP to eth1 because it's no longer managed by the kernel
- Connections will fail with "Connection refused"

You need to test from a **different physical machine** on the same network.

## Stopping the server

Press `Ctrl+C` to gracefully shutdown the server.

## Architecture

```
┌─────────────────────────────────────┐
│  External Client Machine            │
│  (10.0.0.x)                         │
└────────────┬────────────────────────┘
             │
             │ Network (10.0.0.0/24)
             │
┌────────────▼────────────────────────┐
│  Azure VM                           │
│                                     │
│  eth0 (10.0.0.4) - Kernel/SSH      │
│  eth1 (10.0.0.5) - DPDK Server     │
│         │                           │
│         ├─ DPDK PMD (mlx5)         │
│         ├─ smoltcp TCP/IP          │
│         └─ Echo Server (port 8080) │
└─────────────────────────────────────┘
```

## Notes

- The server automatically detects eth1's IP address and gateway
- Uses RSS for multi-queue support (if configured)
- Runs single-threaded poll loop with 100μs sleep
- Prints status updates every 10 seconds when connected
