//! NeoDOS network interface configuration **contract** (#363).
//!
//! This crate is the single definition of the persistent network interface
//! configuration: the canonical Registry value names, the defaulting rules and
//! the IPv4 helpers. It has no kernel or syscall dependency, so it can be unit
//! tested on the host.
//!
//! The syscall-backed accessors (open/read/write/apply) live in
//! `libnet::config`, which re-exports everything here. Consumers (`netcfg`,
//! `dhcpd`, `ipconfig`, `netapplier`, future `neocfg`) must not hardcode
//! `Network\Interfaces\<n>` or its value names.
//!
//! When built for NeoDOS it is `no_std`; under `cargo test` it uses the host
//! standard library.

#![cfg_attr(not(test), no_std)]

/// Registry path prefix for the network interfaces (index appended).
pub const REG_NET_PREFIX: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\";

// Canonical value names (see #314). Do not duplicate these in consumers.
/// `DHCPEnabled` — 1 when the interface is DHCP-managed.
pub const VALUE_DHCP_ENABLED: &str = "DHCPEnabled";
/// `IPAddress` — static/leased IPv4 address.
pub const VALUE_IP_ADDRESS: &str = "IPAddress";
/// `SubnetMask` — IPv4 subnet mask (0 = unset -> `/24`).
pub const VALUE_SUBNET_MASK: &str = "SubnetMask";
/// `Gateway` — default gateway (0 = unset).
pub const VALUE_GATEWAY: &str = "Gateway";
/// `DnsServer` — preferred DNS server (0 = unset).
pub const VALUE_DNS1: &str = "DnsServer";
/// `DnsServer2` — secondary DNS server (0 = unset).
pub const VALUE_DNS2: &str = "DnsServer2";
/// `DnsServer3` — tertiary DNS server (0 = unset).
pub const VALUE_DNS3: &str = "DnsServer3";
/// `DHCPBound` — 1 when a DHCP lease is active.
pub const VALUE_DHCP_BOUND: &str = "DHCPBound";
/// `DHCPServer` — server that granted the lease (0 = unset).
pub const VALUE_DHCP_SERVER: &str = "DHCPServer";
/// `LeaseTime` — lease duration in seconds.
pub const VALUE_LEASE_TIME: &str = "LeaseTime";
/// `LeaseObtained` — Unix seconds when the lease was granted (0 = unknown).
pub const VALUE_LEASE_OBTAINED: &str = "LeaseObtained";
/// `T1Renew` — renewal time (option 58) in seconds (0 = unset).
pub const VALUE_T1_RENEW: &str = "T1Renew";
/// `T2Rebind` — rebind time (option 59) in seconds (0 = unset).
pub const VALUE_T2_REBIND: &str = "T2Rebind";
/// `Domain` — connection-specific DNS suffix (option 15, REG_SZ).
pub const VALUE_DOMAIN: &str = "Domain";
/// `Broadcast` — subnet broadcast address (option 28, 0 = unset).
pub const VALUE_BROADCAST: &str = "Broadcast";
/// `NtpServer` — NTP servers (option 42, 0 = unset).
pub const VALUE_NTP1: &str = "NtpServer";
pub const VALUE_NTP2: &str = "NtpServer2";
pub const VALUE_NTP3: &str = "NtpServer3";
/// `MTU` — interface MTU (option 26, 0 = unset).
pub const VALUE_MTU: &str = "MTU";

/// `/24` (`255.255.255.0`), used when `SubnetMask` is absent or 0.
///
/// NeoDOS represents IPv4 big-endian (`Ipv4Addr::to_u32`), so the mask is
/// `0xFFFF_FF00`. The previous `0x00FF_FFFF` value was byte-swapped (#367).
pub const DEFAULT_MASK: u32 = 0xFFFF_FF00;
/// Human-readable form of a `/24`.
pub const DEFAULT_MASK_STR: &str = "255.255.255.0";

/// Decoded network interface configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetConfig {
    pub dhcp_enabled: bool,
    pub ip: u32,
    pub mask: u32,
    pub gateway: u32,
    pub dns: [u32; 3],
    pub dhcp_bound: bool,
    pub dhcp_server: u32,
    pub lease_time: u32,
    /// Unix seconds when the current lease was granted (0 = unknown).
    pub lease_obtained: u32,
    /// Renewal (T1, option 58) and rebind (T2, option 59) times in seconds
    /// relative to `lease_obtained` (0 = server did not send; #316 defaults).
    pub t1_renew: u32,
    pub t2_rebind: u32,
    /// Connection-specific DNS suffix (option 15), NUL-padded.
    pub domain: [u8; 64],
    /// Subnet broadcast address (option 28, 0 = unset).
    pub broadcast: u32,
    /// NTP servers (option 42, 0 = unset slot).
    pub ntp: [u32; 3],
    /// Interface MTU (option 26, 0 = unset).
    pub mtu: u32,
}

/// Maximum bytes kept from the DHCP domain option (option 15).
pub const DOMAIN_MAX: usize = 63;

impl Default for NetConfig {
    fn default() -> Self {
        NetConfig {
            dhcp_enabled: true,
            ip: 0,
            mask: 0,
            gateway: 0,
            dns: [0; 3],
            dhcp_bound: false,
            dhcp_server: 0,
            lease_time: 0,
            lease_obtained: 0,
            t1_renew: 0,
            t2_rebind: 0,
            domain: [0; 64],
            broadcast: 0,
            ntp: [0; 3],
            mtu: 0,
        }
    }
}

impl NetConfig {
    /// Subnet mask to use for runtime/routing decisions (`/24` when unset).
    pub fn effective_mask(&self) -> u32 {
        if self.mask == 0 { DEFAULT_MASK } else { self.mask }
    }

    /// True when a usable IPv4 address is configured.
    pub fn has_ip(&self) -> bool {
        self.ip != 0
    }

    /// True when the interface is statically configured (DHCP disabled).
    pub fn is_static(&self) -> bool {
        !self.dhcp_enabled
    }

    /// True when the configured gateway is on the interface subnet.
    ///
    /// Returns `false` when the gateway or IP is unset.
    pub fn gateway_on_subnet(&self) -> bool {
        if self.ip == 0 || self.gateway == 0 {
            return false;
        }
        let mask = self.effective_mask();
        (self.gateway & mask) == (self.ip & mask)
    }
}

/// Parse a dotted-decimal IPv4 address into a big-endian `u32`.
pub fn parse_ip(s: &str) -> Option<u32> {
    let mut ip: u32 = 0;
    let mut count = 0usize;
    for part in s.split('.') {
        if count == 4 {
            return None;
        }
        let octet: u32 = part.parse().ok()?;
        if octet > 255 {
            return None;
        }
        ip = (ip << 8) | octet;
        count += 1;
    }
    if count == 4 { Some(ip) } else { None }
}

/// Format a big-endian IPv4 `u32` into `buf`, returning the number of bytes.
pub fn format_ip(ip: u32, buf: &mut [u8]) -> usize {
    let octets = ip.to_be_bytes();
    let mut pos = 0;
    for (i, &octet) in octets.iter().enumerate() {
        if i > 0 {
            if pos < buf.len() { buf[pos] = b'.'; pos += 1; }
        }
        let mut d = [0u8; 3];
        let mut n = 0;
        let mut v = octet as usize;
        loop {
            if n < 3 { d[n] = b'0' + (v % 10) as u8; n += 1; }
            v /= 10;
            if v == 0 { break; }
        }
        for j in (0..n).rev() {
            if pos < buf.len() { buf[pos] = d[j]; pos += 1; }
        }
    }
    pos
}

/// True when `ip` (big-endian u32) is in the APIPA range 169.254.0.0/16.
/// Length of the NUL-terminated domain suffix in `domain`.
pub fn domain_len(domain: &[u8; 64]) -> usize {
    domain.iter().position(|&b| b == 0).unwrap_or(64)
}

pub fn is_apipa(ip: u32) -> bool {
    ip & 0xFFFF_0000 == 0xA9FE_0000
}

/// Build the Registry path for interface `iface` into `buf` (no NUL).
pub fn build_interface_path(iface: u32, buf: &mut [u8]) -> usize {
    let prefix = REG_NET_PREFIX.as_bytes();
    let mut pos = 0;
    let copy = prefix.len().min(buf.len());
    buf[..copy].copy_from_slice(&prefix[..copy]);
    pos += copy;
    if pos >= buf.len() {
        return pos;
    }
    // Decimal digits of `iface`, most-significant first.
    let mut digits = [0u8; 10];
    let mut n = 0;
    let mut v = iface;
    loop {
        digits[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
        if v == 0 { break; }
    }
    for j in (0..n).rev() {
        if pos < buf.len() { buf[pos] = digits[j]; pos += 1; } else { break; }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ip_valid_and_invalid() {        assert_eq!(parse_ip("10.0.1.1"), Some(0x0A00_0101));
        assert_eq!(parse_ip("255.255.255.0"), Some(0xFFFF_FF00));
        assert_eq!(parse_ip("0.0.0.0"), Some(0));
        assert_eq!(parse_ip("10.0.1"), None);
        assert_eq!(parse_ip("10.0.1.256"), None);
        assert_eq!(parse_ip("10.0.1.1.5"), None);
        assert_eq!(parse_ip("a.b.c.d"), None);
    }

    #[test]
    fn format_ip_roundtrip() {
        let mut buf = [0u8; 16];
        let n = format_ip(0x0A00_0101, &mut buf);
        assert_eq!(&buf[..n], b"10.0.1.1");
        let n = format_ip(0, &mut buf);
        assert_eq!(&buf[..n], b"0.0.0.0");
        let n = format_ip(0xC0A8_0101, &mut buf);
        assert_eq!(&buf[..n], b"192.168.1.1");
    }

    #[test]
    fn effective_mask_defaults_to_slash24() {
        let cfg = NetConfig::default();
        assert_eq!(cfg.effective_mask(), DEFAULT_MASK);
        // #367: the default /24 must be 255.255.255.0 in big-endian form.
        assert_eq!(DEFAULT_MASK, 0xFFFF_FF00);
        assert_eq!(parse_ip("255.255.255.0"), Some(DEFAULT_MASK));
        let mut buf = [0u8; 16];
        let n = format_ip(DEFAULT_MASK, &mut buf);
        assert_eq!(&buf[..n], b"255.255.255.0");
        let cfg = NetConfig { mask: 0xFFFF_0000, ..NetConfig::default() };
        assert_eq!(cfg.effective_mask(), 0xFFFF_0000);
    }

    #[test]
    fn gateway_on_subnet() {
        // Explicit /24 (big-endian); see #367 for the legacy DEFAULT_MASK value.
        let mask = 0xFFFF_FF00;
        let on = NetConfig { ip: 0x0A00_0105, gateway: 0x0A00_0101, mask, ..NetConfig::default() };
        assert!(on.gateway_on_subnet());
        let off = NetConfig { ip: 0x0A00_0105, gateway: 0x0A00_0201, mask, ..NetConfig::default() };
        assert!(!off.gateway_on_subnet());
        let none = NetConfig::default();
        assert!(!none.gateway_on_subnet());
    }

    #[test]
    fn interface_path_is_canonical() {
        let mut buf = [0u8; 128];
        let n = build_interface_path(0, &mut buf);
        assert_eq!(
            core::str::from_utf8(&buf[..n]).unwrap(),
            "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0"
        );
        let n = build_interface_path(12, &mut buf);
        assert!(core::str::from_utf8(&buf[..n]).unwrap().ends_with("\\Interfaces\\12"));
    }

    #[test]
    fn value_names_are_canonical() {
        assert_eq!(VALUE_IP_ADDRESS, "IPAddress");
        assert_eq!(VALUE_SUBNET_MASK, "SubnetMask");
        assert_eq!(VALUE_GATEWAY, "Gateway");
        assert_eq!(VALUE_DHCP_ENABLED, "DHCPEnabled");
        assert_eq!(VALUE_DNS1, "DnsServer");
        assert_eq!(VALUE_DNS2, "DnsServer2");
        assert_eq!(VALUE_DNS3, "DnsServer3");
        assert_eq!(VALUE_DHCP_BOUND, "DHCPBound");
        assert_eq!(VALUE_DHCP_SERVER, "DHCPServer");
        assert_eq!(VALUE_LEASE_TIME, "LeaseTime");
        assert_eq!(VALUE_LEASE_OBTAINED, "LeaseObtained");
        assert_eq!(VALUE_T1_RENEW, "T1Renew");
        assert_eq!(VALUE_T2_REBIND, "T2Rebind");
        assert_eq!(VALUE_DOMAIN, "Domain");
        assert_eq!(VALUE_BROADCAST, "Broadcast");
        assert_eq!(VALUE_NTP1, "NtpServer");
        assert_eq!(VALUE_MTU, "MTU");
    }

    #[test]
    fn apipa_range_detection() {
        assert!(is_apipa(0xA9FE_0101)); // 169.254.1.1
        assert!(is_apipa(0xA9FE_FFFF));
        assert!(!is_apipa(0x0A00_0105)); // 10.0.1.5
        assert!(!is_apipa(0x7F00_0001)); // 127.0.0.1
        assert!(!is_apipa(0));
    }

    #[test]
    fn default_config_is_dhcp_with_no_address() {
        let cfg = NetConfig::default();
        assert!(cfg.dhcp_enabled);
        assert!(!cfg.is_static());
        assert!(!cfg.has_ip());
        assert_eq!(cfg.dns, [0, 0, 0]);
    }
}
