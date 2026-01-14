/// Use software loopback for test
#[test]
fn test_smoltcp_on_dpdk_loopback() {
    rpkt_test::util::ensure_hugepages().unwrap();
    rpkt_test::tcp::tcp_echo_test(true);
}
