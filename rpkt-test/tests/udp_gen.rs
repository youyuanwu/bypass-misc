// there is only port 0, so the tests must run serially
#[test]
#[serial_test::serial]
fn udp_gen() {
    // rpkt_test::util::ensure_hugepages().unwrap();
    rpkt_test::send::udp_gen("wtf", 0);
}
