#[test]
#[serial_test::serial]
fn udp_gen2() {
    // dpdk_net_test::util::ensure_hugepages().unwrap();
    dpdk_net_test::send::udp_gen("wtf2", 0);
}
