//! TCP Echo Async Test
//!
//! This test creates an async TCP echo server and client using DPDK with smoltcp.
//! It demonstrates the tcp module's TcpListener and TcpStream APIs.
//!
//! Note: This is a separate test file because DPDK has global state that persists
//! across tests within the same process.

use dpdk_net::tcp::{
    DEFAULT_MBUF_DATA_ROOM_SIZE, DEFAULT_MBUF_HEADROOM, DpdkDeviceWithPool, Reactor, TcpListener,
    TcpStream,
};
use rpkt_dpdk::*;
use smoltcp::iface::{Config, Interface};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};

const SERVER_PORT: u16 = 8080;
const SERVER_IP: Ipv4Address = Ipv4Address::new(192, 168, 1, 1);
const NUM_CLIENTS: usize = 5;

/// Test N clients connecting simultaneously and being served
async fn run_multi_client_test(
    handle: &dpdk_net::tcp::ReactorHandle,
    listener: &mut TcpListener,
    num_clients: usize,
) -> Result<(), &'static str> {
    println!(
        "\n--- Running multi-client test with {} clients ---\n",
        num_clients
    );
    println!("Listener states: {:?}", listener.states());

    // Create all clients at once (they all start connecting simultaneously)
    let mut clients = Vec::with_capacity(num_clients);
    for i in 0..num_clients {
        let local_port = 49152 + i as u16;
        let client = TcpStream::connect(
            handle,
            IpAddress::Ipv4(SERVER_IP),
            SERVER_PORT,
            local_port,
            4096,
            4096,
        )
        .map_err(|_| "client connect failed")?;
        println!("Client {}: created (local port {})", i, local_port);
        clients.push(client);
    }

    println!(
        "\nAll {} clients created, listener states: {:?}",
        num_clients,
        listener.states()
    );

    // Wait for all clients to connect first
    println!("\n=== Waiting for all clients to connect ===");
    for (i, client) in clients.iter().enumerate() {
        client
            .wait_connected()
            .await
            .map_err(|_| "client connection failed")?;
        println!("Client {}: connected", i);
    }
    println!(
        "All clients connected, listener states: {:?}",
        listener.states()
    );

    // Accept connections into a vec - they may be in any order
    println!("\n=== Accepting all connections ===");
    let mut server_streams = Vec::with_capacity(num_clients);
    for i in 0..num_clients {
        let server_stream = listener.accept().await.map_err(|_| "accept failed")?;
        println!("Server: accepted connection {}", i);
        server_streams.push(server_stream);
    }

    // Now we have clients and servers - but they're not matched!
    // Each server_stream is connected to exactly one client, determined by 4-tuple.
    // We'll send from each client, then have each server recv and echo,
    // then have each client recv.

    // But there's no way to know which server_stream corresponds to which client
    // without inspecting the remote endpoint. For the test, let's just verify
    // that ALL echoes work correctly.

    println!("\n=== Echo test (all clients simultaneously) ===");

    // Each client sends a unique message
    for (i, client) in clients.iter().enumerate() {
        let message = format!("Hello from client {}!", i);
        client
            .send(message.as_bytes())
            .await
            .map_err(|_| "client send failed")?;
        println!("Client {}: sent '{}'", i, message);
    }

    // Each server receives and echoes
    for (i, stream) in server_streams.iter().enumerate() {
        let mut buf = [0u8; 1024];
        let len = stream
            .recv(&mut buf)
            .await
            .map_err(|_| "server recv failed")?;
        stream
            .send(&buf[..len])
            .await
            .map_err(|_| "server send failed")?;
        let msg = std::str::from_utf8(&buf[..len]).unwrap_or("<invalid>");
        println!("Server {}: echoed '{}'", i, msg);
    }

    // Each client receives its echo and verifies
    let mut verified = 0;
    for (i, client) in clients.iter().enumerate() {
        let expected = format!("Hello from client {}!", i);
        let mut echo_buf = [0u8; 1024];
        let echo_len = client
            .recv(&mut echo_buf)
            .await
            .map_err(|_| "client recv failed")?;

        let received = std::str::from_utf8(&echo_buf[..echo_len]).map_err(|_| "invalid utf8")?;

        if received != expected {
            println!(
                "Client {}: MISMATCH! expected '{}', got '{}'",
                i, expected, received
            );
            return Err("echo mismatch");
        }
        println!("Client {}: echo verified ✓", i);
        verified += 1;
    }

    println!("\n✓ All {} clients verified!", verified);
    Ok(())
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

    // Create server listener with backlog = NUM_CLIENTS + 1 to handle burst
    // The +1 ensures there's always one socket ready even when all clients connect at once
    let mut listener =
        TcpListener::bind_with_backlog(&handle, SERVER_PORT, 4096, 4096, NUM_CLIENTS + 1)
            .expect("Failed to bind listener");
    println!(
        "Server listening on {}:{} (backlog={})",
        SERVER_IP,
        SERVER_PORT,
        listener.backlog()
    );

    // Run the async test using the reactor's block_on
    let result = reactor.block_on(run_multi_client_test(&handle, &mut listener, NUM_CLIENTS));

    // Verify the result
    match result {
        Ok(()) => {
            println!("\n--- Test Result ---");
            println!(
                "\n✓ TCP Echo Async Test PASSED ({} clients served)!\n",
                NUM_CLIENTS
            );
        }
        Err(e) => {
            panic!("Test failed: {}", e);
        }
    }
}
