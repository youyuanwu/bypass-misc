/// Use software loopback for test
#[test]
#[ignore = "require hardware and driver support"]
fn test_smoltcp_on_dpdk_loopback() {
    dpdk_net_test::util::ensure_hugepages().unwrap();
    dpdk_net_test::tcp::tcp_echo_test(true);
}
