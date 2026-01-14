#[test]
#[serial_test::serial]
fn tcp_send2() {
    rpkt_test::util::ensure_hugepages().unwrap();
    rpkt_test::send::smoltcp_send("wtf2", 0);
}
