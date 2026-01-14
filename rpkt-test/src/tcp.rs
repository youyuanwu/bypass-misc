use crate::dpdk_device::DpdkDeviceWithPool;
use rpkt_dpdk::*;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};
pub fn tcp_echo_test(loop_back_mode: bool) {
    let args = if loop_back_mode {
        "" // use real nic
    } else {
        "--vdev=net_ring0 --no-pci" // virtual ring device only
    };
    // Initialize DPDK with virtual ring device only (no physical NIC binding)
    DpdkOption::new().args(args.split(" ")).init().unwrap();
    // Create mempool
    service()
        .mempool_alloc("tcp_pool", 8192, 256, 2048 + 128, 0)
        .unwrap();

    // Configure port
    let eth_conf = EthConf::new();
    let rxq_confs = vec![RxqConf::new(1024, 0, "tcp_pool")];
    let txq_confs = vec![TxqConf::new(1024, 0)];

    service()
        .dev_configure_and_start(0, &eth_conf, &rxq_confs, &txq_confs)
        .unwrap();

    // Get queues and mempool
    let rxq = service().rx_queue(0, 0).unwrap();
    let txq = service().tx_queue(0, 0).unwrap();
    let mempool = service().mempool("tcp_pool").unwrap();

    // Create DPDK device for smoltcp
    let mut device = DpdkDeviceWithPool::new(rxq, txq, mempool, 1500);

    // Note: Virtual ring devices don't actually support loopback either,
    // so we still need software loopback for self-addressed packets
    if loop_back_mode {
        device.enable_loopback();
    }

    // Configure smoltcp interface
    let config = Config::new(EthernetAddress([0x00, 0x50, 0x56, 0xae, 0x76, 0xf5]).into());
    let mut iface = Interface::new(config, &mut device, Instant::now());

    // Set IP address and enable loopback
    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(
                IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 100)),
                24,
            ))
            .unwrap();
    });

    // Add a route for loopback traffic
    iface
        .routes_mut()
        .add_default_ipv4_route(Ipv4Address::new(192, 168, 1, 1))
        .unwrap();

    // Create socket set with two TCP sockets
    let mut sockets = SocketSet::new(vec![]);

    // Server socket - listens on port 8080
    let server_rx_buffer = tcp::SocketBuffer::new(vec![0; 4096]);
    let server_tx_buffer = tcp::SocketBuffer::new(vec![0; 4096]);
    let mut server_socket = tcp::Socket::new(server_rx_buffer, server_tx_buffer);
    server_socket.listen(8080).unwrap();
    let server_handle = sockets.add(server_socket);

    // Client socket - will connect to the server
    let client_rx_buffer = tcp::SocketBuffer::new(vec![0; 4096]);
    let client_tx_buffer = tcp::SocketBuffer::new(vec![0; 4096]);
    let client_socket = tcp::Socket::new(client_rx_buffer, client_tx_buffer);
    let client_handle = sockets.add(client_socket);

    println!("Smoltcp interface initialized on DPDK");
    println!("IP: 192.168.1.100/24");
    println!("MAC: 00:50:56:ae:76:f5");
    println!("Server listening on port 8080");

    let mut client_connected = false;
    let mut client_sent = false;
    let mut server_echoed = false;
    let mut client_received = false;

    // Poll loop - run for up to 200 iterations to allow TCP handshake
    for iteration in 0..200 {
        let timestamp = Instant::now();
        let poll_result = iface.poll(timestamp, &mut device, &mut sockets);

        // Debug output every 20 iterations
        if iteration % 20 == 0 && iteration > 0 {
            println!(
                "[Debug] Iteration {}: poll_result={:?}",
                iteration, poll_result
            );
        }

        // Client socket logic
        {
            let client = sockets.get_mut::<tcp::Socket>(client_handle);

            if !client_connected && !client.is_open() {
                // Connect to local server
                println!("[Client] Connecting to 192.168.1.100:8080");
                let remote_endpoint = (Ipv4Address::new(192, 168, 1, 100), 8080);
                client
                    .connect(iface.context(), remote_endpoint, 49152)
                    .unwrap();
                client_connected = true;
            } else if client.is_active() && client.may_send() && !client_sent {
                // Send data once connected and ready
                let data = b"Hello, TCP server!";
                if client.send_slice(data).is_ok() {
                    println!("[Client] Sent: {:?}", std::str::from_utf8(data).unwrap());
                    client_sent = true;
                }
            } else if client.may_recv() {
                // Read response from server
                client
                    .recv(|data| {
                        if !data.is_empty() {
                            println!(
                                "[Client] Received: {:?}",
                                std::str::from_utf8(data).unwrap()
                            );
                            client_received = true;
                        }
                        (data.len(), ())
                    })
                    .ok();
            }

            // Debug output every 20 iterations
            if iteration % 20 == 0 && iteration > 0 {
                let state = client.state();
                println!(
                    "[Debug] Iteration {}: Client state={:?}, may_send={}, may_recv={}",
                    iteration,
                    state,
                    client.may_send(),
                    client.may_recv()
                );
            }
        }

        // Server socket logic
        {
            let server = sockets.get_mut::<tcp::Socket>(server_handle);

            // Debug output every 20 iterations
            if iteration % 20 == 0 && iteration > 0 {
                let state = server.state();
                println!(
                    "[Debug] Iteration {}: Server state={:?}, is_listening={}, is_active={}, may_recv={}",
                    iteration,
                    state,
                    server.is_listening(),
                    server.is_active(),
                    server.may_recv()
                );
            }

            if server.may_recv() {
                // Read data from client and collect it
                let mut echo_data = Vec::new();
                let received = server.recv(|data| {
                    if !data.is_empty() {
                        println!(
                            "[Server] Received: {:?}",
                            std::str::from_utf8(data).unwrap()
                        );
                        echo_data.extend_from_slice(data);
                    }
                    (data.len(), ())
                });

                // Echo back the data if we received any
                if received.is_ok() && !echo_data.is_empty() && server.may_send() {
                    if server.send_slice(&echo_data).is_ok() {
                        println!("[Server] Echoed back {} bytes", echo_data.len());
                        server_echoed = true;
                    }
                }
            }
        }

        // Exit early if we've completed the echo cycle
        if server_echoed && client_received && iteration > 10 {
            println!("Echo test completed successfully!");
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Assert that the full echo cycle completed
    assert!(client_sent, "Client failed to send data");
    assert!(server_echoed, "Server failed to echo data back");
    assert!(client_received, "Client failed to receive echoed data");

    // Cleanup
    drop(device);
    drop(sockets);
    drop(iface);

    service().dev_stop_and_close(0).unwrap();
    service().mempool_free("tcp_pool").unwrap();
    service().graceful_cleanup().unwrap();
}
