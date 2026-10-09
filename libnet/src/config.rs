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
//! Registry access goes through the shared `libneodos::registry::RegistryKey`
//! client (no ad-hoc `[type][len][data]` parsing, #588). Consumers must not
//! hardcode `Network\Interfaces\<n>` or its value names (#314); the persistent
//! store remains the Registry interface key.
//!
//! The pure contract (canonical names, defaults, IPv4 helpers, [`NetConfig`])
//! lives in the dependency-free, host-testable `libnet-config` crate and is
//! re-exported here.

use libneodos::registry::RegistryKey;
use libneodos::syscall;

pub use libnet_config::*;

use crate::NetIfaceInfo;

fn interface_key(iface: u32) -> Result<RegistryKey, i64> {
    let mut buf = [0u8; 128];
    let n = build_interface_path(iface, &mut buf);
    let path = core::str::from_utf8(&buf[..n]).map_err(|_| -1i64)?;
    RegistryKey::open(path)
}

/// Ensure the Registry key for interface `iface` exists, creating
/// `Interfaces\<iface>` under the Network key when missing.
pub fn ensure_interface(iface: u32) -> Result<(), i64> {
    let mut buf = [0u8; 128];
    let n = build_interface_path(iface, &mut buf);
    let path = core::str::from_utf8(&buf[..n]).map_err(|_| -1i64)?;
    RegistryKey::create_tree(path).map(|_| ())
}

/// Open the Registry key for interface `iface`.
pub fn open_interface(iface: u32) -> Result<RegistryKey, i64> {
    interface_key(iface)
}

/// Current time as Unix seconds (0 = unknown).
///
/// Shared by `dhcpd` (lease stamps) and `ipconfig` (expiry checks).
pub fn now_unix() -> u32 {
    use libneodos::syscall::{DateTime, ObInfoClass};
    let fd = match syscall::sys_ob_open("\\Global\\Info\\DateTime", 1) {
        Ok(fd) => fd,
        Err(_) => return 0,
    };
    let mut dt = DateTime {
        second: 0, minute: 0, hour: 0,
        day: 0, month: 0, year: 0, valid: 0,
    };
    let sz = core::mem::size_of::<DateTime>();
    let buf = unsafe {
        core::slice::from_raw_parts_mut(&mut dt as *mut DateTime as *mut u8, sz)
    };
    let n = syscall::sys_ob_query_info(fd, ObInfoClass::DateTime, buf);
    let _ = syscall::sys_close(fd);
    if n.ok().unwrap_or(0) < sz || dt.valid == 0 {
        return 0;
    }
    let utc = libntp::UtcDateTime {
        second: dt.second, minute: dt.minute, hour: dt.hour,
        day: dt.day, month: dt.month, year: dt.year,
    };
    if !libntp::is_valid_datetime(&utc) {
        return 0;
    }
    libntp::utc_to_unix_secs(&utc).clamp(0, u32::MAX as i64) as u32
}

/// Load the full interface configuration from the Registry.
///
/// Returns `None` when the interface key cannot be opened.
pub fn load(iface: u32) -> Option<NetConfig> {
    let key = interface_key(iface).ok()?;
    Some(load_key(&key))
}

/// Load the full interface configuration from an open key.
pub fn load_key(key: &RegistryKey) -> NetConfig {
    NetConfig {
        dhcp_enabled: key.query_dword(VALUE_DHCP_ENABLED).unwrap_or(1) != 0,
        ip: key.query_dword(VALUE_IP_ADDRESS).unwrap_or(0),
        mask: key.query_dword(VALUE_SUBNET_MASK).unwrap_or(0),
        gateway: key.query_dword(VALUE_GATEWAY).unwrap_or(0),
        dns: [
            key.query_dword(VALUE_DNS1).unwrap_or(0),
            key.query_dword(VALUE_DNS2).unwrap_or(0),
            key.query_dword(VALUE_DNS3).unwrap_or(0),
        ],
        dhcp_bound: key.query_dword(VALUE_DHCP_BOUND).unwrap_or(0) != 0,
        dhcp_server: key.query_dword(VALUE_DHCP_SERVER).unwrap_or(0),
        lease_time: key.query_dword(VALUE_LEASE_TIME).unwrap_or(0),
        lease_obtained: key.query_dword(VALUE_LEASE_OBTAINED).unwrap_or(0),
        t1_renew: key.query_dword(VALUE_T1_RENEW).unwrap_or(0),
        t2_rebind: key.query_dword(VALUE_T2_REBIND).unwrap_or(0),
        domain: {
            let mut d = [0u8; 64];
            key.query_string(VALUE_DOMAIN, &mut d);
            d
        },
        broadcast: key.query_dword(VALUE_BROADCAST).unwrap_or(0),
        ntp: [
            key.query_dword(VALUE_NTP1).unwrap_or(0),
            key.query_dword(VALUE_NTP2).unwrap_or(0),
            key.query_dword(VALUE_NTP3).unwrap_or(0),
        ],
        mtu: key.query_dword(VALUE_MTU).unwrap_or(0),
    }
}

/// Write the full interface configuration to an already-open key.
pub fn store_key(key: &RegistryKey, cfg: &NetConfig) {
    let _ = key.set_dword(VALUE_DHCP_ENABLED, cfg.dhcp_enabled as u32);
    let _ = key.set_dword(VALUE_IP_ADDRESS, cfg.ip);
    let _ = key.set_dword(VALUE_SUBNET_MASK, cfg.mask);
    let _ = key.set_dword(VALUE_GATEWAY, cfg.gateway);
    let _ = key.set_dword(VALUE_DNS1, cfg.dns[0]);
    let _ = key.set_dword(VALUE_DNS2, cfg.dns[1]);
    let _ = key.set_dword(VALUE_DNS3, cfg.dns[2]);
    let _ = key.set_dword(VALUE_DHCP_BOUND, cfg.dhcp_bound as u32);
    let _ = key.set_dword(VALUE_DHCP_SERVER, cfg.dhcp_server);
    let _ = key.set_dword(VALUE_LEASE_TIME, cfg.lease_time);
    let _ = key.set_dword(VALUE_LEASE_OBTAINED, cfg.lease_obtained);
    let _ = key.set_dword(VALUE_T1_RENEW, cfg.t1_renew);
    let _ = key.set_dword(VALUE_T2_REBIND, cfg.t2_rebind);
    let _ = key.set_string(VALUE_DOMAIN, &cfg.domain[..domain_len(&cfg.domain)]);
    let _ = key.set_dword(VALUE_BROADCAST, cfg.broadcast);
    let _ = key.set_dword(VALUE_NTP1, cfg.ntp[0]);
    let _ = key.set_dword(VALUE_NTP2, cfg.ntp[1]);
    let _ = key.set_dword(VALUE_NTP3, cfg.ntp[2]);
    let _ = key.set_dword(VALUE_MTU, cfg.mtu);
}

/// Write the full interface configuration and flush it to disk.
pub fn store(iface: u32, cfg: &NetConfig) -> Result<(), i64> {
    let key = interface_key(iface)?;
    store_key(&key, cfg);
    key.flush()
}

/// Publish a DHCP lease (or APIPA fallback) to the Registry.
///
/// This is the `dhcpd` path: it writes the lease values and marks the
/// interface as bound. It never touches the runtime NIC (the applier does).
pub fn publish_lease(iface: u32, cfg: &NetConfig) -> Result<(), i64> {
    let key = interface_key(iface)?;
    let _ = key.set_dword(VALUE_IP_ADDRESS, cfg.ip);
    let _ = key.set_dword(VALUE_SUBNET_MASK, cfg.mask);
    let _ = key.set_dword(VALUE_GATEWAY, cfg.gateway);
    let _ = key.set_dword(VALUE_DNS1, cfg.dns[0]);
    let _ = key.set_dword(VALUE_DNS2, cfg.dns[1]);
    let _ = key.set_dword(VALUE_DNS3, cfg.dns[2]);
    let _ = key.set_dword(VALUE_LEASE_TIME, cfg.lease_time);
    let _ = key.set_dword(VALUE_LEASE_OBTAINED, cfg.lease_obtained);
    let _ = key.set_dword(VALUE_T1_RENEW, cfg.t1_renew);
    let _ = key.set_dword(VALUE_T2_REBIND, cfg.t2_rebind);
    let _ = key.set_string(VALUE_DOMAIN, &cfg.domain[..domain_len(&cfg.domain)]);
    let _ = key.set_dword(VALUE_BROADCAST, cfg.broadcast);
    let _ = key.set_dword(VALUE_NTP1, cfg.ntp[0]);
    let _ = key.set_dword(VALUE_NTP2, cfg.ntp[1]);
    let _ = key.set_dword(VALUE_NTP3, cfg.ntp[2]);
    let _ = key.set_dword(VALUE_MTU, cfg.mtu);
    let _ = key.set_dword(VALUE_DHCP_BOUND, cfg.dhcp_bound as u32);
    if cfg.dhcp_server != 0 {
        let _ = key.set_dword(VALUE_DHCP_SERVER, cfg.dhcp_server);
    }
    key.flush()
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
