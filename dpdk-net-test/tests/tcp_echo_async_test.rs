//! TCP Echo Async Test
//!
//! This test creates an async TCP echo server and client using DPDK with smoltcp.
//! It demonstrates the async_net module's Reactor and AsyncTcpSocket APIs.
//!
//! Note: This is a separate test file because DPDK has global state that persists
//! across tests within the same process.

use dpdk_net::async_net::{AsyncTcpSocket, Reactor};
use dpdk_net::tcp::{DEFAULT_MBUF_DATA_ROOM_SIZE, DEFAULT_MBUF_HEADROOM, DpdkDeviceWithPool};
use rpkt_dpdk::*;
use smoltcp::iface::{Config, Interface};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};

const SERVER_PORT: u16 = 8080;
const SERVER_IP: Ipv4Address = Ipv4Address::new(192, 168, 1, 1);
const TEST_MESSAGE: &[u8] = b"Hello, Async Echo Server!";

/// Combined async test that runs both server and client
async fn run_async_echo_test(
    server_socket: AsyncTcpSocket,
    client_socket: AsyncTcpSocket,
    message: &[u8],
) -> Result<Vec<u8>, &'static str> {
    // In a real scenario, these would run concurrently with proper task spawning.
    // For this simple test, we interleave them manually by leveraging the
    // single-threaded nature - the reactor polls both sockets.

    // The client sends first, server receives and echoes, client receives
    println!("\n--- Running async echo test ---\n");

    // First, wait for client to connect
    println!("Client state: {:?}", client_socket.state());
    println!("Server state: {:?}", server_socket.state());

    client_socket
        .wait_connected()
        .await
        .map_err(|_| "client connection failed")?;
    println!(
        "Client: connected to server, state: {:?}",
        client_socket.state()
    );
    println!(
        "Server state after client connect: {:?}",
        server_socket.state()
    );

    // Wait for server to accept the connection (transition from Listen/SynReceived to Established)
    // Use wait_connected which handles all pre-established states
    server_socket
        .wait_connected()
        .await
        .map_err(|_| "server accept failed")?;
    println!(
        "Server: accepted connection, state: {:?}",
        server_socket.state()
    );

    // Client sends the message
    client_socket
        .send(message)
        .await
        .map_err(|_| "client send failed")?;
    println!("Client: sent {} bytes", message.len());

    // Server receives the message
    let mut server_buf = [0u8; 1024];
    let server_len = server_socket
        .recv(&mut server_buf)
        .await
        .map_err(|_| "server recv failed")?;
    println!("Server: received {} bytes", server_len);

    // Server echoes it back
    server_socket
        .send(&server_buf[..server_len])
        .await
        .map_err(|_| "server send failed")?;
    println!("Server: echoed {} bytes", server_len);

    // Client receives the echo
    let mut client_buf = [0u8; 1024];
    let client_len = client_socket
        .recv(&mut client_buf)
        .await
        .map_err(|_| "client recv failed")?;
    println!("Client: received echo of {} bytes", client_len);

    Ok(client_buf[..client_len].to_vec())
}

#[test]
fn test_tcp_echo_async() {
    println!("\n=== TCP Echo Async Test ===\n");

    // Initialize DPDK with virtual ring device
    DpdkOption::new()
        .args(["--no-huge", "--no-pci", "--vdev=net_ring0"])
        .init()
        .unwrap();

    // Create mempool
    service()
        .mempool_alloc(
            "async_test_pool",
            8192,
            256,
            DEFAULT_MBUF_DATA_ROOM_SIZE as u16,
            0,
        )
        .unwrap();

    // Configure port with 1 queue pair
    let eth_conf = EthConf::new();
    let rxq_confs = vec![RxqConf::new(1024, 0, "async_test_pool")];
    let txq_confs = vec![TxqConf::new(1024, 0)];

    service()
        .dev_configure_and_start(0, &eth_conf, &rxq_confs, &txq_confs)
        .unwrap();

    // Get queue and mempool
    let rxq = service().rx_queue(0, 0).unwrap();
    let txq = service().tx_queue(0, 0).unwrap();
    let mempool = service().mempool("async_test_pool").unwrap();

    // Create DPDK device
    let mbuf_capacity = DEFAULT_MBUF_DATA_ROOM_SIZE - DEFAULT_MBUF_HEADROOM;
    let mut device = DpdkDeviceWithPool::new(rxq, txq, mempool, 1500, mbuf_capacity);

    // Configure smoltcp interface
    let mac_addr = EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    let config = Config::new(mac_addr.into());
    let mut iface = Interface::new(config, &mut device, Instant::now());

    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(IpAddress::Ipv4(SERVER_IP), 24))
            .unwrap();
    });

    // Create the async reactor
    let reactor = Reactor::new(device, iface);
    let handle = reactor.handle();

    // Create server socket (listening)
    let server_socket = AsyncTcpSocket::listen(&handle, SERVER_PORT, 4096, 4096)
        .expect("Failed to create listening socket");
    println!("Server listening on {}:{}", SERVER_IP, SERVER_PORT);

    // Create client socket (connecting)
    let client_socket = AsyncTcpSocket::connect(
        &handle,
        IpAddress::Ipv4(SERVER_IP),
        SERVER_PORT,
        49152,
        4096,
        4096,
    )
    .expect("Failed to create client socket");
    println!("Client connecting to {}:{}", SERVER_IP, SERVER_PORT);

    // Run the async test using the reactor's block_on
    let result = reactor.block_on(run_async_echo_test(
        server_socket,
        client_socket,
        TEST_MESSAGE,
    ));

    // Verify the result
    match result {
        Ok(echoed) => {
            println!("\n--- Test Result ---");
            println!("Sent:     {:?}", TEST_MESSAGE);
            println!("Received: {:?}", &echoed);
            assert_eq!(echoed, TEST_MESSAGE, "Echo mismatch!");
            println!("\n✓ TCP Echo Async Test PASSED!\n");
        }
        Err(e) => {
            panic!("Test failed: {}", e);
        }
    }
}
