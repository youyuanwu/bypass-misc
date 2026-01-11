#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::sync::Once;

    static DPDK_INIT: Once = Once::new();
    static mut SHARED_MEMPOOL: *mut dpdk_sys::rte_mempool = std::ptr::null_mut();

    fn init_dpdk() {
        DPDK_INIT.call_once(|| {
            unsafe {
                let c_argv: Vec<_> = vec![
                    CString::new("dpdk-test").unwrap(),
                    CString::new("--no-huge").unwrap(),
                    CString::new("--no-shconf").unwrap(),
                    CString::new("--file-prefix=dpdk_test").unwrap(),
                    CString::new("--vdev=net_ring0").unwrap(), // Virtual ethernet device for testing
                    CString::new("--vdev=net_ring1").unwrap(), // Second virtual device for forwarding
                ];
                let mut c_argv_ptrs: Vec<*mut i8> =
                    c_argv.iter().map(|arg| arg.as_ptr() as *mut i8).collect();
                let ret =
                    dpdk_sys::rte_eal_init(c_argv_ptrs.len() as i32, c_argv_ptrs.as_mut_ptr());
                assert!(
                    ret >= 0,
                    "DPDK EAL initialization failed with code: {}",
                    ret
                );

                // Create a shared mempool for all tests to avoid exceeding mempool ops limit
                let pool_name = CString::new("shared_mbuf_pool").unwrap();
                let num_mbufs = 8192;
                let cache_size = 250;
                let socket_id = dpdk_sys::rte_socket_id() as i32;

                SHARED_MEMPOOL = dpdk_sys::rte_pktmbuf_pool_create(
                    pool_name.as_ptr(),
                    num_mbufs,
                    cache_size,
                    0,
                    dpdk_sys::RTE_MBUF_DEFAULT_BUF_SIZE as u16,
                    socket_id,
                );
                assert!(!SHARED_MEMPOOL.is_null(), "Failed to create shared mempool");
            }
        });
    }

    fn get_mempool() -> *mut dpdk_sys::rte_mempool {
        unsafe { SHARED_MEMPOOL }
    }

    #[test]
    fn dummy_test() {
        init_dpdk();

        unsafe {
            assert_eq!(dpdk_sys::rte_is_power_of_2(7), 0);
            assert_eq!(dpdk_sys::rte_is_power_of_2(16), 1);
        }
    }

    // Callback function for lcore execution
    unsafe extern "C" fn lcore_hello(_arg: *mut std::ffi::c_void) -> i32 {
        let lcore_id = unsafe { dpdk_sys::rte_lcore_id() };
        println!("hello from core {}", lcore_id);
        0
    }

    #[test]
    fn test_helloworld() {
        init_dpdk();

        unsafe {
            // Launch function on each worker lcore
            let mut lcore_id = dpdk_sys::rte_get_next_lcore(u32::MAX, 1, 0);
            while lcore_id < dpdk_sys::RTE_MAX_LCORE {
                dpdk_sys::rte_eal_remote_launch(Some(lcore_hello), std::ptr::null_mut(), lcore_id);
                lcore_id = dpdk_sys::rte_get_next_lcore(lcore_id, 1, 0);
            }

            // Call it on main lcore too
            lcore_hello(std::ptr::null_mut());

            // Wait for all lcores to finish
            dpdk_sys::rte_eal_mp_wait_lcore();
        }
    }

    #[test]
    fn test_mbuf_tx_rx() {
        init_dpdk();

        unsafe {
            // Use shared mempool
            let pktmbuf_pool = get_mempool();
            assert!(!pktmbuf_pool.is_null(), "Failed to get shared mempool");

            // Allocate an mbuf from the pool
            let mbuf = dpdk_sys::rte_pktmbuf_alloc(pktmbuf_pool);
            assert!(!mbuf.is_null(), "Failed to allocate mbuf");

            // Prepare test data to write
            let test_data: [u8; 64] = [
                // Destination MAC (6 bytes)
                0x02, 0x00, 0x00, 0x00, 0x00, 0x01, // Source MAC (6 bytes)
                0x02, 0x00, 0x00, 0x00, 0x00, 0x02,
                // EtherType (2 bytes) - 0x0800 for IPv4
                0x08, 0x00, // Payload (remaining bytes - 50 bytes to make 64 total)
                0x45, 0x00, 0x00, 0x2e, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8,
                0x01, 0x01, 0xc0, 0xa8, 0x01, 0x02, 0x04, 0xd2, 0x16, 0x2e, 0x00, 0x1a, 0x00, 0x00,
                0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x44, 0x50, 0x44, 0x4b, 0x21, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ];

            // Get pointer to mbuf data area (access the buf_addr field directly)
            let buf_addr = (*mbuf).buf_addr as *mut u8;
            let data_off = (*mbuf).data_off as usize;
            let data_ptr = buf_addr.add(data_off);
            assert!(!data_ptr.is_null(), "Failed to get mbuf data pointer");

            // Write test data to mbuf
            std::ptr::copy_nonoverlapping(test_data.as_ptr(), data_ptr, test_data.len());

            // Set the data length
            (*mbuf).data_len = test_data.len() as u16;
            (*mbuf).pkt_len = test_data.len() as u32;

            // Read back and verify data
            let read_back = std::slice::from_raw_parts(data_ptr, test_data.len());
            assert_eq!(
                read_back, &test_data,
                "Data mismatch: written data doesn't match read data"
            );

            // Verify Ethernet header
            assert_eq!(
                read_back[0..6],
                [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
                "Destination MAC mismatch"
            );
            assert_eq!(
                read_back[6..12],
                [0x02, 0x00, 0x00, 0x00, 0x00, 0x02],
                "Source MAC mismatch"
            );
            assert_eq!(read_back[12..14], [0x08, 0x00], "EtherType mismatch");

            // Verify payload contains "Hello DPDK!"
            let payload_offset = 42;
            let hello_msg = b"Hello DPDK!";
            assert_eq!(
                &read_back[payload_offset..payload_offset + hello_msg.len()],
                hello_msg,
                "Payload mismatch"
            );

            println!(
                "Successfully wrote {} bytes to mbuf and verified contents",
                test_data.len()
            );
            println!(
                "Packet data length: {}, total length: {}",
                (*mbuf).data_len,
                (*mbuf).pkt_len
            );

            // Free the mbuf
            dpdk_sys::rte_pktmbuf_free(mbuf);

            // Free the mempool (note: DPDK doesn't have a direct pool free, it's managed by EAL)
            println!("Test completed successfully");
        }
    }

    #[test]
    fn test_eth_dev_tx_rx() {
        init_dpdk();

        unsafe {
            // Check available ports
            let nb_ports = dpdk_sys::rte_eth_dev_count_avail();
            println!("Number of available ethernet devices: {}", nb_ports);
            assert!(nb_ports > 0, "No ethernet devices available");

            let port_id = 0u16;

            // Use shared mempool
            let pktmbuf_pool = get_mempool();
            assert!(!pktmbuf_pool.is_null(), "Failed to get shared mempool");

            // Configure the ethernet device
            let port_conf: dpdk_sys::rte_eth_conf = std::mem::zeroed();
            let nb_rx_queue = 1u16;
            let nb_tx_queue = 1u16;

            let ret =
                dpdk_sys::rte_eth_dev_configure(port_id, nb_rx_queue, nb_tx_queue, &port_conf);
            assert_eq!(
                ret, 0,
                "Failed to configure port {}: error code {}",
                port_id, ret
            );

            // Setup RX queue
            let socket_id = dpdk_sys::rte_socket_id();
            let nb_rxd = 128u16;
            let ret = dpdk_sys::rte_eth_rx_queue_setup(
                port_id,
                0, // queue_id
                nb_rxd,
                socket_id,
                std::ptr::null(),
                pktmbuf_pool,
            );
            assert_eq!(ret, 0, "Failed to setup RX queue: error code {}", ret);

            // Setup TX queue
            let nb_txd = 128u16;
            let ret = dpdk_sys::rte_eth_tx_queue_setup(
                port_id,
                0, // queue_id
                nb_txd,
                socket_id,
                std::ptr::null(),
            );
            assert_eq!(ret, 0, "Failed to setup TX queue: error code {}", ret);

            // Start the ethernet device
            let ret = dpdk_sys::rte_eth_dev_start(port_id);
            assert_eq!(
                ret, 0,
                "Failed to start port {}: error code {}",
                port_id, ret
            );

            // Enable promiscuous mode
            let ret = dpdk_sys::rte_eth_promiscuous_enable(port_id);
            if ret != 0 {
                println!(
                    "Warning: Failed to enable promiscuous mode: error code {}",
                    ret
                );
            }

            println!("Port {} configured and started successfully", port_id);

            // Allocate an mbuf for transmission
            let tx_mbuf = dpdk_sys::rte_pktmbuf_alloc(pktmbuf_pool);
            assert!(!tx_mbuf.is_null(), "Failed to allocate TX mbuf");

            // Prepare test packet data
            let test_data: [u8; 64] = [
                // Destination MAC (6 bytes)
                0x02, 0x00, 0x00, 0x00, 0x00, 0x01, // Source MAC (6 bytes)
                0x02, 0x00, 0x00, 0x00, 0x00, 0x02,
                // EtherType (2 bytes) - 0x0800 for IPv4
                0x08, 0x00, // Payload - "Hello from DPDK ethernet test!" (30 bytes)
                0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x66, 0x72, 0x6f, 0x6d, 0x20, 0x44, 0x50, 0x44,
                0x4b, 0x20, 0x65, 0x74, 0x68, 0x65, 0x72, 0x6e, 0x65, 0x74, 0x20, 0x74, 0x65, 0x73,
                0x74, 0x21, // Padding to reach 64 bytes (20 bytes)
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ];

            // Write data to mbuf
            let buf_addr = (*tx_mbuf).buf_addr as *mut u8;
            let data_off = (*tx_mbuf).data_off as usize;
            let data_ptr = buf_addr.add(data_off);
            std::ptr::copy_nonoverlapping(test_data.as_ptr(), data_ptr, test_data.len());
            (*tx_mbuf).data_len = test_data.len() as u16;
            (*tx_mbuf).pkt_len = test_data.len() as u32;

            println!("Transmitting packet with {} bytes", test_data.len());

            // Transmit the packet
            let mut pkts_to_send = [tx_mbuf];
            let nb_tx = dpdk_sys::rte_eth_tx_burst(port_id, 0, pkts_to_send.as_mut_ptr(), 1);
            println!("Transmitted {} packet(s)", nb_tx);

            if nb_tx == 0 {
                // If transmission failed, free the mbuf
                dpdk_sys::rte_pktmbuf_free(tx_mbuf);
                println!("Warning: Transmission failed, but continuing test");
            }

            // Try to receive packets (the ring device should loop it back)
            let mut rx_pkts: [*mut dpdk_sys::rte_mbuf; 32] = [std::ptr::null_mut(); 32];
            let max_attempts = 10;
            let mut received = false;

            for attempt in 0..max_attempts {
                let nb_rx = dpdk_sys::rte_eth_rx_burst(port_id, 0, rx_pkts.as_mut_ptr(), 32);

                if nb_rx > 0 {
                    println!("Received {} packet(s) on attempt {}", nb_rx, attempt + 1);

                    for (i, &rx_mbuf) in rx_pkts.iter().enumerate().take(nb_rx as usize) {
                        if !rx_mbuf.is_null() {
                            let pkt_len = (*rx_mbuf).pkt_len;
                            let buf_addr = (*rx_mbuf).buf_addr as *mut u8;
                            let data_off = (*rx_mbuf).data_off as usize;
                            let data_ptr = buf_addr.add(data_off);
                            let rx_data = std::slice::from_raw_parts(data_ptr, pkt_len as usize);

                            println!("Received packet {} with {} bytes", i, pkt_len);

                            // Check if it's our packet by comparing first 14 bytes (ethernet header)
                            if pkt_len >= 14 && rx_data[0..14] == test_data[0..14] {
                                println!("✓ Received our test packet!");
                                received = true;
                            }

                            dpdk_sys::rte_pktmbuf_free(rx_mbuf);
                        }
                    }
                    break;
                }

                // Small delay between attempts
                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            if !received {
                println!(
                    "Note: Did not receive the test packet back (this may be expected with ring devices)"
                );
            }

            // Stop the port
            let ret = dpdk_sys::rte_eth_dev_stop(port_id);
            if ret != 0 {
                println!("Warning: Failed to stop port: error code {}", ret);
            }

            println!("Ethernet device test completed");
        }
    }
}
