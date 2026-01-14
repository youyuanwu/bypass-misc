//! DPDK TCP Echo Server using smoltcp
//!
//! This example starts a TCP server on eth1 using DPDK+smoltcp.
//! It listens on port 8080 and echoes back any data received.
//!
//! Usage:
//!   sudo -E cargo run --example dpdk_tcp_server
//!
//! Then from another machine on the same network:
//!   nc 10.0.0.5 8080
//!   # Type messages and see them echoed back

use rpkt_dpdk::*;
use rpkt_test::dpdk_device::DpdkDeviceWithPool;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};
use std::thread;
use std::time::Duration;

fn main() {
    // Setup hugepages
    rpkt_test::util::ensure_hugepages().unwrap();

    // Get network configuration for eth1
    let interface = "eth1";
    let ip_addr =
        rpkt_test::tcp::get_interface_ipv4(interface).expect("Failed to get IP address for eth1");
    let gateway = rpkt_test::tcp::get_default_gateway().unwrap_or(Ipv4Address::new(10, 0, 0, 1));

    println!("\n========================================");
    println!("DPDK TCP Echo Server");
    println!("========================================");
    println!("IP Address: {}:8080", ip_addr);
    println!("Gateway: {}", gateway);
    println!("\nServer is starting...\n");

    // Get PCI address for eth1
    let pci_addr =
        rpkt_test::tcp::get_pci_addr(interface).expect("Failed to get PCI address for eth1");
    let args = format!("-a {}", pci_addr);

    // Initialize DPDK
    DpdkOption::new()
        .args(&args.split(" ").collect::<Vec<_>>())
        .init()
        .unwrap();

    // Create mempool
    service()
        .mempool_alloc("server_pool", 8192, 256, 2048 + 128, 0)
        .unwrap();

    // Configure port
    let eth_conf = EthConf::new();
    let rxq_confs = vec![RxqConf::new(1024, 0, "server_pool")];
    let txq_confs = vec![TxqConf::new(1024, 0)];

    service()
        .dev_configure_and_start(0, &eth_conf, &rxq_confs, &txq_confs)
        .unwrap();

    // Get queues and mempool
    let rxq = service().rx_queue(0, 0).unwrap();
    let txq = service().tx_queue(0, 0).unwrap();
    let mempool = service().mempool("server_pool").unwrap();

    // Create DPDK device for smoltcp
    let mut device = DpdkDeviceWithPool::new(rxq, txq, mempool, 1500);

    // Get MAC address from DPDK
    let dev_info = service().dev_info(0).unwrap();
    let mac_addr = EthernetAddress(dev_info.mac_addr);

    // Configure smoltcp interface
    let config = Config::new(mac_addr.into());
    let mut iface = Interface::new(config, &mut device, Instant::now());

    // Set IP address
    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(IpAddress::Ipv4(ip_addr), 24))
            .unwrap();
    });

    // Add default route
    iface.routes_mut().add_default_ipv4_route(gateway).unwrap();

    println!("Interface configured:");
    println!("  IP: {}/24", ip_addr);
    println!("  MAC: {:?}", mac_addr);
    println!("  Gateway: {}", gateway);

    // Create socket set
    let mut sockets = SocketSet::new(vec![]);

    // Create server socket
    let server_rx_buffer = tcp::SocketBuffer::new(vec![0; 4096]);
    let server_tx_buffer = tcp::SocketBuffer::new(vec![0; 4096]);
    let mut server_socket = tcp::Socket::new(server_rx_buffer, server_tx_buffer);
    server_socket.listen(8080).unwrap();
    let server_handle = sockets.add(server_socket);

    println!("\n✓ Server listening on {}:8080", ip_addr);
    println!("\nConnect from another machine:");
    println!("  nc {} 8080", ip_addr);
    println!("\nPress Ctrl+C to stop the server\n");
    println!("========================================\n");

    // Setup Ctrl+C handler
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\n\nReceived Ctrl+C, shutting down...");
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    })
    .expect("Error setting Ctrl+C handler");

    // Poll loop
    let mut iteration = 0u64;
    let start_time = std::time::Instant::now();
    let mut last_status_time = start_time;

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        let timestamp = Instant::now();
        iface.poll(timestamp, &mut device, &mut sockets);

        let server = sockets.get_mut::<tcp::Socket>(server_handle);

        // Check if we have an active connection
        if server.is_active() {
            // Print connection status every 10 seconds
            let now = std::time::Instant::now();
            if now.duration_since(last_status_time).as_secs() >= 10 {
                println!(
                    "[{}] Connection active (uptime: {}s)",
                    format_time(),
                    start_time.elapsed().as_secs()
                );
                last_status_time = now;
            }

            // Check if we can receive data
            if server.can_recv() {
                let data = server
                    .recv(|buffer| {
                        let len = buffer.len();
                        if len > 0 {
                            let data = buffer.to_vec();
                            (len, data)
                        } else {
                            (0, vec![])
                        }
                    })
                    .unwrap();

                if !data.is_empty() {
                    let data_str = String::from_utf8_lossy(&data);
                    println!(
                        "[{}] [RX] {} bytes: {:?}",
                        format_time(),
                        data.len(),
                        data_str.trim()
                    );

                    // Echo back immediately
                    if server.can_send() {
                        server.send_slice(&data).unwrap();
                        println!("[{}] [TX] Echoed {} bytes", format_time(), data.len());
                    }
                }
            }
        } else if iteration % 10000 == 0 {
            // Print waiting message every ~1 second
            println!(
                "[{}] Waiting for connections... (uptime: {}s)",
                format_time(),
                start_time.elapsed().as_secs()
            );
        }

        iteration += 1;
        thread::sleep(Duration::from_micros(100));
    }

    // Cleanup
    println!("\nCleaning up...");
    drop(device);
    drop(sockets);
    drop(iface);

    service().dev_stop_and_close(0).unwrap();
    service().mempool_free("server_pool").unwrap();
    service().graceful_cleanup().unwrap();

    println!("Server stopped.");
}

fn format_time() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let secs = now.as_secs() % 86400; // Seconds since midnight
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}
