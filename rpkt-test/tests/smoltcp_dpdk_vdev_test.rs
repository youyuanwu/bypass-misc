/// Use DPDK vdev for test
#[test]
fn test_smoltcp_on_dpdk_vdev() {
    rpkt_test::util::ensure_hugepages().unwrap();
    rpkt_test::tcp::tcp_echo_test(false);
}
