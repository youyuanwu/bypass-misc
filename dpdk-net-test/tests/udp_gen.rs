// there is only port 0, so the tests must run serially
#[test]
#[serial_test::serial]
fn udp_gen() {
    // dpdk_net_test::util::ensure_hugepages().unwrap();
    dpdk_net_test::send::udp_gen("wtf", 0);
}
