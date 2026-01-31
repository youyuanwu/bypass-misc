//! DPDK device and ARP cache implementations for smoltcp.
//!
//! This module provides:
//! - [`DpdkDevice`]: A smoltcp `Device` implementation backed by DPDK RX/TX queues
//! - [`SharedArpCache`]: Thread-safe ARP cache for multi-queue DPDK setups
//!
//! # Multi-Queue ARP Sharing
//!
//! When using multiple RX queues with RSS, ARP replies may arrive on a different
//! queue than the one needing the MAC address. The [`SharedArpCache`] solves this
//! by providing a shared cache that all queues can read from.
//!
//! # Usage Pattern
//!
//! 1. Create a [`SharedArpCache`] and share it between queues
//! 2. Create [`DpdkDevice`] for each queue, passing the shared cache
//! 3. Queue 0 will update the cache when it receives ARP replies
//! 4. Other queues will check the cache and inject ARP packets into smoltcp

mod arp_cache;
mod dpdk_device;

pub use arp_cache::{MacAddress, SharedArpCache, build_arp_reply_for_injection, parse_arp_reply};
pub use dpdk_device::*;
