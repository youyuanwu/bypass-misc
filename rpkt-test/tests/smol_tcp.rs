// there is only port 0, so the tests must run serially
#[test]
#[serial_test::serial]
fn tcp_send() {
    rpkt_test::util::ensure_hugepages().unwrap();
    rpkt_test::send::smoltcp_send("wtf", 0);
}
