//! Shared network configuration backend / config API (#363).
//!
//! This is the single path through which userland reads and writes the network
//! interface configuration stored in the Registry, and the single place that
//! knows the canonical value names and the "apply to NIC" step:
//!
//! ```text
//! neocfg / netcfg / dhcpd / ipconfig / netapplier
//!                       │
//!                       ▼
//!               libnet::config  (this module)
//!                  │         │
//!        Registry (Cm)    net.nxl SetNicIp/SetNicGateway
//! ```
//!
//! Consumers must not hardcode `Network\Interfaces\<n>` or its value names.
//! The persistent store remains the Registry interface key (canonical names per
//! #314); there is no second source of truth.
//!
//! The pure contract (canonical names, defaults, IPv4 helpers, [`NetConfig`])
//! lives in the dependency-free, host-testable `libnet-config` crate and is
//! re-exported here.

use libneodos::syscall;

pub use libnet_config::*;

use crate::NetIfaceInfo;

/// Open the Registry key for interface `iface`.
pub fn open_interface(iface: u32) -> Result<u8, i64> {
    let mut buf = [0u8; 128];
    let n = libnet_config::build_interface_path(iface, &mut buf);
    let path = core::str::from_utf8(&buf[..n]).map_err(|_| -1i64)?;
    syscall::sys_cm_open_key(path)
}

/// Read a REG_DWORD value from an open interface key.
pub fn read_dword(key_fd: u8, name: &str) -> Option<u32> {
    let mut reg_buf = [0u8; 12];
    let total = syscall::sys_cm_query_value(key_fd, name, &mut reg_buf).ok()?;
    if total < 12 {
        return None;
    }
    let value_type = u32::from_le_bytes([reg_buf[0], reg_buf[1], reg_buf[2], reg_buf[3]]);
    if value_type != syscall::REG_DWORD {
        return None;
    }
    Some(u32::from_le_bytes([reg_buf[8], reg_buf[9], reg_buf[10], reg_buf[11]]))
}

/// Write a REG_DWORD value to an open interface key.
pub fn write_dword(key_fd: u8, name: &str, val: u32) {
    let _ = syscall::sys_cm_set_value(key_fd, name, syscall::REG_DWORD, &val.to_le_bytes());
}

/// Read a REG_SZ value into `buf`, returning the byte length (without NUL).
pub fn read_string(key_fd: u8, name: &str, buf: &mut [u8]) -> usize {
    let mut reg = [0u8; 264];
    let total = match syscall::sys_cm_query_value(key_fd, name, &mut reg) {
        Ok(n) => n,
        Err(_) => return 0,
    };
    if total < 8 {
        return 0;
    }
    let data_len = u32::from_le_bytes([reg[4], reg[5], reg[6], reg[7]]) as usize;
    let available = total.saturating_sub(8).min(reg.len() - 8);
    let src = &reg[8..8 + data_len.min(available)];
    let end = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    let n = end.min(buf.len());
    buf[..n].copy_from_slice(&src[..n]);
    n
}

/// Write a REG_SZ value (raw bytes, no NUL added).
pub fn write_string(key_fd: u8, name: &str, val: &[u8]) {
    let _ = syscall::sys_cm_set_value(key_fd, name, syscall::REG_SZ, val);
}

/// Load the full interface configuration from the Registry.
///
/// Returns `None` when the interface key cannot be opened.
pub fn load(iface: u32) -> Option<NetConfig> {
    let fd = open_interface(iface).ok()?;
    let cfg = load_fd(fd);
    let _ = syscall::sys_close(fd);
    Some(cfg)
}

/// Load the full interface configuration from an already-open key.
pub fn load_fd(fd: u8) -> NetConfig {
    NetConfig {
        dhcp_enabled: read_dword(fd, VALUE_DHCP_ENABLED).unwrap_or(1) != 0,
        ip: read_dword(fd, VALUE_IP_ADDRESS).unwrap_or(0),
        mask: read_dword(fd, VALUE_SUBNET_MASK).unwrap_or(0),
        gateway: read_dword(fd, VALUE_GATEWAY).unwrap_or(0),
        dns: [
            read_dword(fd, VALUE_DNS1).unwrap_or(0),
            read_dword(fd, VALUE_DNS2).unwrap_or(0),
            read_dword(fd, VALUE_DNS3).unwrap_or(0),
        ],
        dhcp_bound: read_dword(fd, VALUE_DHCP_BOUND).unwrap_or(0) != 0,
        dhcp_server: read_dword(fd, VALUE_DHCP_SERVER).unwrap_or(0),
        lease_time: read_dword(fd, VALUE_LEASE_TIME).unwrap_or(0),
        lease_obtained: read_dword(fd, VALUE_LEASE_OBTAINED).unwrap_or(0),
        t1_renew: read_dword(fd, VALUE_T1_RENEW).unwrap_or(0),
        t2_rebind: read_dword(fd, VALUE_T2_REBIND).unwrap_or(0),
        domain: {
            let mut d = [0u8; 64];
            read_string(fd, VALUE_DOMAIN, &mut d);
            d
        },
        broadcast: read_dword(fd, VALUE_BROADCAST).unwrap_or(0),
        ntp: [
            read_dword(fd, VALUE_NTP1).unwrap_or(0),
            read_dword(fd, VALUE_NTP2).unwrap_or(0),
            read_dword(fd, VALUE_NTP3).unwrap_or(0),
        ],
        mtu: read_dword(fd, VALUE_MTU).unwrap_or(0),
    }
}

/// Write the full interface configuration to an already-open key.
pub fn store_fd(fd: u8, cfg: &NetConfig) {
    write_dword(fd, VALUE_DHCP_ENABLED, cfg.dhcp_enabled as u32);
    write_dword(fd, VALUE_IP_ADDRESS, cfg.ip);
    write_dword(fd, VALUE_SUBNET_MASK, cfg.mask);
    write_dword(fd, VALUE_GATEWAY, cfg.gateway);
    write_dword(fd, VALUE_DNS1, cfg.dns[0]);
    write_dword(fd, VALUE_DNS2, cfg.dns[1]);
    write_dword(fd, VALUE_DNS3, cfg.dns[2]);
    write_dword(fd, VALUE_DHCP_BOUND, cfg.dhcp_bound as u32);
    write_dword(fd, VALUE_DHCP_SERVER, cfg.dhcp_server);
    write_dword(fd, VALUE_LEASE_TIME, cfg.lease_time);
    write_dword(fd, VALUE_LEASE_OBTAINED, cfg.lease_obtained);
    write_dword(fd, VALUE_T1_RENEW, cfg.t1_renew);
    write_dword(fd, VALUE_T2_REBIND, cfg.t2_rebind);
    write_string(fd, VALUE_DOMAIN, &cfg.domain[..libnet_config::domain_len(&cfg.domain)]);
    write_dword(fd, VALUE_BROADCAST, cfg.broadcast);
    write_dword(fd, VALUE_NTP1, cfg.ntp[0]);
    write_dword(fd, VALUE_NTP2, cfg.ntp[1]);
    write_dword(fd, VALUE_NTP3, cfg.ntp[2]);
    write_dword(fd, VALUE_MTU, cfg.mtu);
}

/// Write the full interface configuration and flush it to disk.
pub fn store(iface: u32, cfg: &NetConfig) -> Result<(), i64> {
    let fd = open_interface(iface)?;
    store_fd(fd, cfg);
    let r = syscall::sys_cm_flush_key(fd);
    let _ = syscall::sys_close(fd);
    r
}

/// Publish a DHCP lease (or APIPA fallback) to the Registry.
///
/// This is the `dhcpd` path: it writes the lease values and marks the
/// interface as bound. It never touches the runtime NIC (the applier does).
pub fn publish_lease(iface: u32, cfg: &NetConfig) -> Result<(), i64> {
    let fd = open_interface(iface)?;
    write_dword(fd, VALUE_IP_ADDRESS, cfg.ip);
    write_dword(fd, VALUE_SUBNET_MASK, cfg.mask);
    write_dword(fd, VALUE_GATEWAY, cfg.gateway);
    write_dword(fd, VALUE_DNS1, cfg.dns[0]);
    write_dword(fd, VALUE_DNS2, cfg.dns[1]);
    write_dword(fd, VALUE_DNS3, cfg.dns[2]);
    write_dword(fd, VALUE_LEASE_TIME, cfg.lease_time);
    write_dword(fd, VALUE_LEASE_OBTAINED, cfg.lease_obtained);
    write_dword(fd, VALUE_T1_RENEW, cfg.t1_renew);
    write_dword(fd, VALUE_T2_REBIND, cfg.t2_rebind);
    write_string(fd, VALUE_DOMAIN, &cfg.domain[..libnet_config::domain_len(&cfg.domain)]);
    write_dword(fd, VALUE_BROADCAST, cfg.broadcast);
    write_dword(fd, VALUE_NTP1, cfg.ntp[0]);
    write_dword(fd, VALUE_NTP2, cfg.ntp[1]);
    write_dword(fd, VALUE_NTP3, cfg.ntp[2]);
    write_dword(fd, VALUE_MTU, cfg.mtu);
    write_dword(fd, VALUE_DHCP_BOUND, cfg.dhcp_bound as u32);
    if cfg.dhcp_server != 0 {
        write_dword(fd, VALUE_DHCP_SERVER, cfg.dhcp_server);
    }
    let r = syscall::sys_cm_flush_key(fd);
    let _ = syscall::sys_close(fd);
    r
}

/// Apply a configuration to the runtime NIC (`SetNicIp`/`SetNicGateway`).
pub fn apply(iface: u32, cfg: &NetConfig) {
    crate::set_ip(iface, cfg.ip, cfg.effective_mask());
    if cfg.gateway != 0 {
        crate::set_gateway(iface, cfg.gateway);
    }
}

/// Load the Registry config and apply it to the NIC if it is static.
///
/// No-op under DHCP (the `netapplier` lease path handles that). Returns the
/// loaded configuration, or `None` when the key cannot be opened.
pub fn apply_current(iface: u32) -> Option<NetConfig> {
    let cfg = load(iface)?;
    if !cfg.dhcp_enabled && cfg.has_ip() {
        apply(iface, &cfg);
    }
    Some(cfg)
}

/// Link state (`link_up`) of interface `iface`, via net.nxl `NicInfo`.
pub fn link_up(iface: u32) -> u8 {
    let mut info = blank_iface_info();
    if crate::iface_info(iface, &mut info) == 0 {
        info.link_up
    } else {
        0
    }
}

/// Runtime IPv4 address of interface `iface` (0 when unavailable).
pub fn interface_ip(iface: u32) -> u32 {
    let mut info = blank_iface_info();
    if crate::iface_info(iface, &mut info) == 0 {
        u32::from_be_bytes(info.ip)
    } else {
        0
    }
}

fn blank_iface_info() -> NetIfaceInfo {
    NetIfaceInfo {
        nic_id: 0,
        mac: [0u8; 6],
        ip: [0u8; 4],
        link_up: 0,
        vendor_id: 0,
        device_id: 0,
        name: [0u8; 16],
        description: [0u8; 48],
    }
}
