#[test]
#[serial_test::serial]
fn udp_gen2() {
    // rpkt_test::util::ensure_hugepages().unwrap();
    rpkt_test::send::udp_gen("wtf2", 0);
}
