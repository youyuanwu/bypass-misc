// Test: UDP socket binding and basic operations
//
// This demonstrates how to use the UdpSocket with DPDK.

use rpkt_dpdk::*;
use rpkt_test::udp::{Endpoint, UdpSocket};
use std::net::Ipv4Addr;

#[test]
fn test_udp_socket_basic() {
    // Initialize DPDK
    DpdkOption::new()
        .args(["--no-huge", "--no-pci", "--vdev=net_ring0"])
        .init()
        .unwrap();

    // Create mempool
    service()
        .mempool_alloc("udp_pool", 8192, 256, 2048 + 128, 0)
        .unwrap();

    // Configure port
    let eth_conf = EthConf::new();
    let rxq_confs = vec![RxqConf::new(1024, 0, "udp_pool")];
    let txq_confs = vec![TxqConf::new(1024, 0)];

    service()
        .dev_configure_and_start(0, &eth_conf, &rxq_confs, &txq_confs)
        .unwrap();

    // Get queues and mempool
    let rxq = service().rx_queue(0, 0).unwrap();
    let txq = service().tx_queue(0, 0).unwrap();
    let mempool = service().mempool("udp_pool").unwrap();

    // Create and configure UDP socket
    let mut socket = UdpSocket::new();
    socket.set_local_mac([0x00, 0x50, 0x56, 0xae, 0x76, 0xf5]);

    // Test binding
    assert!(
        socket
            .bind(Endpoint::new(Ipv4Addr::new(192, 168, 1, 100), 8080))
            .is_ok()
    );

    assert!(socket.is_bound());
    assert_eq!(
        socket.local_endpoint(),
        Some(Endpoint::new(Ipv4Addr::new(192, 168, 1, 100), 8080))
    );

    // Attach queues
    socket.attach_queues(rxq, txq, mempool).unwrap();

    // Test send capability
    assert!(socket.can_send());

    // Test sending a packet
    let remote_mac = [0x00, 0x0b, 0x86, 0x64, 0x8b, 0xa0];
    let remote_endpoint = Endpoint::new(Ipv4Addr::new(192, 168, 1, 200), 9000);
    let test_data = b"Hello, UDP!";

    assert!(
        socket
            .send_to(test_data, remote_endpoint, remote_mac)
            .is_ok()
    );
    assert_eq!(socket.tx_pending(), 1);

    // Flush and verify
    let sent = socket.flush().unwrap();
    assert!(sent > 0);

    // Poll for packets (won't receive any in test environment, but should not error)
    let result = socket.poll();
    assert!(result.is_ok());

    // Cleanup
    drop(socket);
    service().dev_stop_and_close(0).unwrap();
    service().mempool_free("udp_pool").unwrap();
    service().graceful_cleanup().unwrap();
}
