//! ntpd — NeoDOS NTP/SNTP synchronization daemon.
//!
//! Architecture (see `docs/services/ntpd.md`):
//!
//! ```text
//!   neocfg / Registry          ntpd (this daemon)             System Clock
//!   Ntpd\Parameters   ───────► load config, resolve servers   \Global\Info\DateTime
//!                              query NTP/SNTP (UDP 123)  ───► ob_set_datetime()
//!                              publish status to Registry
//! ```
//!
//! ntpd is a persistent Ring 3 service started by the kernel Service Manager
//! (service key `Services\Ntpd`, StartType=Auto), exactly like `dhcpd`. It never
//! configures the system: it only reads the Registry and applies time. It
//! tolerates missing network/DNS/servers and retries with exponential backoff.
//!
//! Known limitations (tracked as Issues):
//! * the clock is applied with a direct step; there is no slew/drift discipline
//!   and no smoothing.
//! * there is no timed sleep in userland, so periodic waits use an RDTSC budget
//!   (same workaround as the DNS resolver).
//! * the RTC has one-second resolution, which bounds offset accuracy.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};

use libneodos::syscall::{self, DateTime, ObInfoClass, ObSetInfoClass, REG_DWORD, REG_SZ};
use libneodos::mem;

struct SbrkAlloc;

unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() { core::ptr::null_mut() } else { ptr }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // bump allocator cannot free; memory is reclaimed on process exit
    }
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

// ── Registry paths ──

const REG_PARAM: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Ntpd\\Parameters";
const REG_SVC: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Ntpd";
const REG_STATUS: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Ntpd\\Status";

// ── Defaults (used when the Parameters key is absent) ──

const DEFAULT_SERVER: &str = "pool.ntp.org";
const DEFAULT_INTERVAL: u32 = 3600;
const DEFAULT_TIMEOUT_MS: u32 = 3000;
const MIN_INTERVAL: u32 = 16;
const INITIAL_BACKOFF_SECS: u32 = 5;
const MAX_BACKOFF_SECS: u32 = 300;
/// Conservative TSC ticks-per-millisecond budget. The exact frequency is not
/// exposed to userland; this under-estimates on typical 2–3 GHz parts, so waits
/// are slightly short — acceptable until a timed-wait API exists (#283/#307).
const TICKS_PER_MS: u64 = 2_000_000;
const YIELD_BATCH: u32 = 64;

// ── Output helpers ──

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u32(mut v: u32) {
    let mut buf = [0u8; 10];
    let mut i = 9;
    if v == 0 {
        write_str(b"0");
        return;
    }
    while v > 0 && i > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..=9]);
}

fn write_i64(mut v: i64) {
    if v < 0 {
        write_str(b"-");
        v = -v;
    }
    let mut buf = [0u8; 20];
    let mut i = 19;
    if v == 0 {
        write_str(b"0");
        return;
    }
    while v > 0 && i > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..=19]);
}

fn write_ip(ip: [u8; 4]) {
    for (i, o) in ip.iter().enumerate() {
        if i > 0 {
            write_str(b".");
        }
        write_u32(*o as u32);
    }
}

// ── Timing ──

fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Wait approximately `secs` seconds by spinning on the TSC while yielding.
fn wait_seconds(secs: u32) {
    let budget = secs as u64 * 1000 * TICKS_PER_MS;
    let start = rdtsc();
    loop {
        if rdtsc().wrapping_sub(start) >= budget {
            break;
        }
        for _ in 0..YIELD_BATCH {
            let _ = syscall::sys_yield();
        }
    }
}

// ── Clock ──

/// Read the current system time as nanoseconds since the Unix epoch.
fn rtc_now_ns() -> Option<i128> {
    let fd = syscall::sys_ob_open("\\Global\\Info\\DateTime", 1).ok()?;
    let mut dt = DateTime {
        second: 0, minute: 0, hour: 0,
        day: 0, month: 0, year: 0, valid: 0,
    };
    let sz = core::mem::size_of::<DateTime>();
    let buf = unsafe { core::slice::from_raw_parts_mut(&mut dt as *mut DateTime as *mut u8, sz) };
    let n = syscall::sys_ob_query_info(fd, ObInfoClass::DateTime, buf);
    let _ = syscall::sys_close(fd);
    if n.ok()? < sz || dt.valid == 0 {
        return None;
    }
    let utc = libntp::UtcDateTime {
        second: dt.second, minute: dt.minute, hour: dt.hour,
        day: dt.day, month: dt.month, year: dt.year,
    };
    Some(libntp::utc_to_unix_secs(&utc) as i128 * 1_000_000_000)
}

/// Apply an absolute UTC time (Unix seconds) to the system clock.
fn apply_time(secs: i64) -> Result<(), &'static str> {
    let utc = libntp::unix_secs_to_utc(secs).ok_or("time out of representable range")?;
    let dt = DateTime {
        second: utc.second, minute: utc.minute, hour: utc.hour,
        day: utc.day, month: utc.month, year: utc.year, valid: 1,
    };
    syscall::ob_set_datetime(&dt).map_err(|_| "clock set denied")
}

// ── Registry helpers ──

fn read_reg_dword(fd: u8, name: &str) -> Option<u32> {
    let mut buf = [0u8; 12];
    let total = syscall::sys_cm_query_value(fd, name, &mut buf).ok()?;
    if total < 12 {
        return None;
    }
    let value_type = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if value_type != REG_DWORD {
        return None;
    }
    Some(u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]))
}

fn read_reg_string(fd: u8, name: &str) -> Option<String> {
    let mut buf = [0u8; 256];
    let total = syscall::sys_cm_query_value(fd, name, &mut buf).ok()?;
    if total < 8 {
        return None;
    }
    let data_len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    let available = total.saturating_sub(8).min(buf.len() - 8);
    let data_len = data_len.min(available);
    let data = &buf[8..8 + data_len];
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    core::str::from_utf8(&data[..end]).ok().map(String::from)
}

fn write_reg_dword(fd: u8, name: &str, val: u32) {
    let _ = syscall::sys_cm_set_value(fd, name, REG_DWORD, &val.to_le_bytes());
}

fn write_reg_string(fd: u8, name: &str, val: &str) {
    let _ = syscall::sys_cm_set_value(fd, name, REG_SZ, val.as_bytes());
}

// ── Configuration ──

struct NtpConfig {
    enabled: bool,
    servers: Vec<String>,
    interval: u32,
    timeout_ms: u32,
}

fn load_config() -> NtpConfig {
    let mut cfg = NtpConfig {
        enabled: true,
        servers: alloc::vec![String::from(DEFAULT_SERVER)],
        interval: DEFAULT_INTERVAL,
        timeout_ms: DEFAULT_TIMEOUT_MS,
    };

    if let Ok(fd) = syscall::sys_cm_open_key(REG_PARAM) {
        if let Some(v) = read_reg_dword(fd, "Enabled") {
            cfg.enabled = v != 0;
        }
        if let Some(s) = read_reg_string(fd, "Servers") {
            let servers = libntp::parse_servers_owned(&s);
            if !servers.is_empty() {
                cfg.servers = servers;
            }
        }
        if let Some(v) = read_reg_dword(fd, "Interval") {
            cfg.interval = v.max(MIN_INTERVAL);
        }
        if let Some(v) = read_reg_dword(fd, "Timeout") {
            cfg.timeout_ms = v.max(100);
        }
        let _ = syscall::sys_close(fd);
    }

    cfg
}

// ── Status ──

/// Open the `\Status` subkey, creating it on first use.
fn status_key() -> Option<u8> {
    if let Ok(fd) = syscall::sys_cm_open_key(REG_STATUS) {
        return Some(fd);
    }
    let svc = syscall::sys_cm_open_key(REG_SVC).ok()?;
    let _ = syscall::sys_ob_set_info(svc, ObSetInfoClass::RegistryCreateKey, b"Status\0");
    let _ = syscall::sys_close(svc);
    syscall::sys_cm_open_key(REG_STATUS).ok()
}

#[allow(clippy::too_many_arguments)]
fn publish(
    state: &str,
    server: &str,
    offset_ms: i64,
    delay_ms: i64,
    stratum: u8,
    last_sync: i64,
    last_error: &str,
    sync_count: u32,
) {
    let Some(fd) = status_key() else { return };
    write_reg_string(fd, "State", state);
    write_reg_string(fd, "Server", server);
    write_reg_dword(fd, "OffsetMs", offset_ms as i32 as u32);
    write_reg_dword(fd, "DelayMs", delay_ms.max(0) as u32);
    write_reg_dword(fd, "Stratum", stratum as u32);
    write_reg_dword(fd, "LastSync", last_sync.max(0) as u32);
    write_reg_string(fd, "LastError", last_error);
    write_reg_dword(fd, "SyncCount", sync_count);
    let _ = syscall::sys_cm_flush_key(fd);
    let _ = syscall::sys_close(fd);
}

// ── Server resolution ──

fn dns_err_str(e: libnet::dns::DnsError) -> &'static str {
    use libnet::dns::DnsError::*;
    match e {
        NoConfig => "dns: no config",
        NoServer => "dns: no server",
        InvalidHostname => "dns: invalid hostname",
        Timeout => "dns: timeout",
        ServerUnreachable => "dns: server unreachable",
        NxDomain => "dns: nxdomain",
        MalformedResponse => "dns: malformed response",
        Truncated => "dns: truncated response",
        ServerFailure => "dns: server failure",
        NoARecord => "dns: no A record",
        Network => "dns: network unavailable",
    }
}

fn resolve_server(name: &str) -> Result<[u8; 4], &'static str> {
    if let Some(ip) = libnet::dns::parse_dotted_ip(name) {
        return Ok(ip);
    }
    match libnet::dns::resolve(name) {
        Ok(r) => r.first().ok_or("dns: no A record"),
        Err(e) => Err(dns_err_str(e)),
    }
}

// ── NTP exchange ──

struct SyncResult {
    offset_ns: i128,
    delay_ns: i128,
    stratum: u8,
    server: [u8; 4],
    /// Corrected absolute UTC time (Unix seconds).
    unix_secs: i64,
}

fn ntp_sync(ip: [u8; 4], timeout_ms: u32) -> Result<SyncResult, &'static str> {
    let fd = libnet::socket_create(2); // SocketType::Udp
    if fd < 0 {
        return Err("socket create failed");
    }
    if libnet::socket_bind(fd, 0, 0) < 0 {
        let _ = libnet::socket_close(fd);
        return Err("socket bind failed");
    }
    if libnet::socket_connect(fd, u32::from_be_bytes(ip), libntp::NTP_PORT) < 0 {
        let _ = libnet::socket_close(fd);
        return Err("socket connect failed");
    }

    let t1 = rtc_now_ns().unwrap_or(0);
    let t1_secs = (t1 / 1_000_000_000) as i64;
    let request = libntp::build_request(libntp::unix_secs_to_ntp(t1_secs));

    // ARP may still be resolving; retry the send like the DNS resolver does.
    let mut sent = false;
    for _ in 0..50 {
        if libnet::socket_send(fd, &request) >= 0 {
            sent = true;
            break;
        }
        let _ = syscall::sys_yield();
        for _ in 0..2000 {
            core::hint::spin_loop();
        }
    }
    if !sent {
        let _ = libnet::socket_close(fd);
        return Err("send failed");
    }

    let mut buf = [0u8; libntp::NTP_PACKET_SIZE];
    let budget = timeout_ms as u64 * TICKS_PER_MS;
    let start = rdtsc();
    let mut received = 0usize;
    loop {
        let n = libnet::socket_recv(fd, &mut buf);
        if n > 0 {
            received = n as usize;
            break;
        }
        if rdtsc().wrapping_sub(start) >= budget {
            break;
        }
        let _ = syscall::sys_yield();
    }
    let _ = libnet::socket_close(fd);

    if received == 0 {
        return Err("timeout");
    }

    let reply = libntp::parse_reply(&buf[..received], libntp::unix_secs_to_ntp(t1_secs))
        .map_err(|_| "invalid NTP response")?;
    let t2 = libntp::ntp_to_unix_ns(reply.receive_ts);
    let t3 = libntp::ntp_to_unix_ns(reply.transmit_ts);
    let t4 = rtc_now_ns().unwrap_or(t1);
    let (offset_ns, delay_ns) = libntp::offset_and_delay(t1, t2, t3, t4);
    let unix_secs = (t4 + offset_ns).div_euclid(1_000_000_000) as i64;

    Ok(SyncResult {
        offset_ns,
        delay_ns,
        stratum: reply.stratum,
        server: ip,
        unix_secs,
    })
}

// ── Main ──

/// Whether the network is usable for an NTP exchange.
///
/// An applied, non-zero interface IP is the readiness signal: it means the
/// stack has a source address and a route. `link_up` is intentionally *not*
/// required — some virtual NICs (e.g. QEMU SLiRP) report link-down while the
/// interface is fully usable, and a genuinely dead link simply makes the
/// exchange fail and fall into backoff.
fn network_ready() -> bool {
    let mut iface = libnet::NetIfaceInfo {
        nic_id: 0,
        mac: [0u8; 6],
        ip: [0u8; 4],
        link_up: 0,
        vendor_id: 0,
        device_id: 0,
        name: [0u8; 16],
        description: [0u8; 48],
    };
    libnet::iface_info(0, &mut iface) == 0 && iface.ip != [0u8; 4]
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n[ntpd] NeoDOS NTP Service v0.1\r\n");

    let cfg = load_config();
    if !cfg.enabled {
        write_str(b"[ntpd] NTP disabled by configuration; idling\r\n");
        publish("Disabled", "", 0, 0, 0, 0, "", 0);
        loop {
            let _ = syscall::sys_yield();
        }
    }

    let mut backoff = INITIAL_BACKOFF_SECS;
    let mut sync_count: u32 = 0;

    loop {
        if !network_ready() {
            write_str(b"[ntpd] network not ready; waiting\r\n");
            publish("WaitingNetwork", "", 0, 0, 0, 0, "network unavailable", sync_count);
            wait_seconds(backoff);
            backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
            continue;
        }

        let mut synced = false;
        for server in &cfg.servers {
            let ip = match resolve_server(server) {
                Ok(ip) => ip,
                Err(e) => {
                    write_str(b"[ntpd] resolve failed: ");
                    write_str(e.as_bytes());
                    write_str(b"\r\n");
                    publish("Error", server, 0, 0, 0, 0, e, sync_count);
                    continue;
                }
            };

            write_str(b"[ntpd] querying ");
            write_str(server.as_bytes());
            write_str(b" (");
            write_ip(ip);
            write_str(b")\r\n");

            match ntp_sync(ip, cfg.timeout_ms) {
                Ok(r) => {
                    let offset_ms = (r.offset_ns / 1_000_000) as i64;
                    let delay_ms = (r.delay_ns / 1_000_000) as i64;
                    match apply_time(r.unix_secs) {
                        Ok(()) => {
                            sync_count += 1;
                            write_str(b"[ntpd] synced: offset=");
                            write_i64(offset_ms);
                            write_str(b"ms delay=");
                            write_i64(delay_ms);
                            write_str(b"ms stratum=");
                            write_u32(r.stratum as u32);
                            write_str(b"\r\n");
                            publish("Synced", server, offset_ms, delay_ms, r.stratum,
                                    r.unix_secs, "", sync_count);
                            let _ = r.server;
                            synced = true;
                            break;
                        }
                        Err(e) => {
                            write_str(b"[ntpd] clock set failed: ");
                            write_str(e.as_bytes());
                            write_str(b"\r\n");
                            publish("Error", server, offset_ms, delay_ms, r.stratum,
                                    r.unix_secs, e, sync_count);
                        }
                    }
                }
                Err(e) => {
                    write_str(b"[ntpd] sync failed: ");
                    write_str(e.as_bytes());
                    write_str(b"\r\n");
                    publish("Error", server, 0, 0, 0, 0, e, sync_count);
                }
            }
        }

        if synced {
            backoff = INITIAL_BACKOFF_SECS;
            wait_seconds(cfg.interval);
        } else {
            write_str(b"[ntpd] all servers failed; backoff ");
            write_u32(backoff);
            write_str(b"s\r\n");
            wait_seconds(backoff);
            backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
        }
    }
}
