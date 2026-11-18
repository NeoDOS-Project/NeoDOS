//! `CfgPlatform` seam over `libneodos::syscall`.
//!
//! This is the only place in the binary that talks to the kernel. Every call
//! goes through the public `libneodos` wrappers — no raw `int 0x80`.

use alloc::{string::String, vec::Vec};
use core::mem::size_of;

use libneocfg::{
    AboutInfo, CertEntry, CertImport, CertOps, CertRejection, CfgError, CfgPlatform, CpuInfo,
    DriveInfo, LocaleOps, MemInfo, PowerOps, ServiceInfo, VersionInfo,
};
use libneodos::i18n;
use libneodos::keyboard;
use libneodos::syscall::{
    self, ob_access, MemInfo as SysMemInfo, ObEnumEntry, ObInfoClass,
};

/// Syscall ABI revision (`docs/kernel/syscalls.md`).
const SYSCALL_ABI: u32 = 8;
/// Target architecture.
const ARCH: &str = "x86_64";
/// On-disk filesystem format (`docs/filesystem/neofs-v2.md`).
const NEOFS_VERSION: &str = "NE2 v2";

/// An 8-byte-aligned byte buffer for structured `ob_query_info` results.
#[repr(C, align(8))]
struct Aligned<const N: usize>([u8; N]);

/// NeoDOS-backed [`CfgPlatform`].
pub struct NeodosPlatform {
    locale: Option<NeodosLocale>,
    certs: NeodosCerts,
}

impl NeodosPlatform {
    pub fn new() -> Self {
        // The i18n runtime (libneodos::i18n) is implemented, so locale ops are
        // available. The Power Manager plan API is not exposed by libneodos yet,
        // so `power()` stays `None` until #326 wires it.
        NeodosPlatform {
            locale: Some(NeodosLocale),
            certs: NeodosCerts,
        }
    }
}

impl Default for NeodosPlatform {
    fn default() -> Self {
        Self::new()
    }
}

fn empty_entries() -> [ObEnumEntry; 64] {
    core::array::from_fn(|_| ObEnumEntry {
        id: 0,
        obj_type: 0,
        name: [0; 32],
        mode: 0,
        _pad: [0; 2],
        size: 0,
    })
}

fn trim(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

impl CfgPlatform for NeodosPlatform {
    fn version(&self) -> Result<VersionInfo, CfgError> {
        let fd = syscall::sys_ob_open("\\Global\\Info\\Version", ob_access::READ)?;
        let mut buf = [0u8; 256];
        let n = match syscall::sys_ob_query_info(fd, ObInfoClass::Version, &mut buf) {
            Ok(n) => n,
            Err(e) => {
                let _ = syscall::sys_close(fd);
                return Err(CfgError::from(e));
            }
        };
        let _ = syscall::sys_close(fd);
        let n = n.min(buf.len());
        Ok(VersionInfo {
            version: String::from_utf8_lossy(trim(&buf[..n])).into_owned(),
        })
    }

    fn about(&self) -> Result<AboutInfo, CfgError> {
        Ok(AboutInfo {
            neodos_version: self.version()?.version,
            syscall_abi: SYSCALL_ABI,
            arch: String::from(ARCH),
            neofs: String::from(NEOFS_VERSION),
            libneodos_abi: libneodos::export::ABI_VERSION,
            // The kernel does not expose a build date through the Ob namespace;
            // the row is omitted until one exists.
            build_date: None,
        })
    }

    fn memory(&self) -> Result<MemInfo, CfgError> {
        let fd = syscall::sys_ob_open("\\Global\\Info\\Memory", ob_access::READ)?;
        let mut buf = Aligned([0u8; 256]);
        let n = match syscall::sys_ob_query_info(fd, ObInfoClass::Memory, &mut buf.0) {
            Ok(n) => n,
            Err(e) => {
                let _ = syscall::sys_close(fd);
                return Err(CfgError::from(e));
            }
        };
        let _ = syscall::sys_close(fd);
        if n < size_of::<SysMemInfo>() {
            return Err(CfgError::Io(-1));
        }
        let m = unsafe { core::ptr::read_unaligned(buf.0.as_ptr() as *const SysMemInfo) };
        Ok(MemInfo {
            total_kib: m.total_kib,
            used_kib: m.used_kib,
            free_kib: m.free_kib,
        })
    }

    fn cpu(&self) -> Result<CpuInfo, CfgError> {
        let fd = syscall::sys_ob_open("\\Global\\Info\\CpuInfo", ob_access::READ)?;
        let mut buf = Aligned([0u8; 256]);
        let n = match syscall::sys_ob_query_info(fd, ObInfoClass::CpuInfo, &mut buf.0) {
            Ok(n) => n,
            Err(e) => {
                let _ = syscall::sys_close(fd);
                return Err(CfgError::from(e));
            }
        };
        let _ = syscall::sys_close(fd);
        if n < size_of::<syscall::CpuInfoFull>() {
            return Err(CfgError::Io(-1));
        }
        let c = unsafe { core::ptr::read_unaligned(buf.0.as_ptr() as *const syscall::CpuInfoFull) };
        Ok(CpuInfo {
            vendor: String::from(c.vendor_str()),
            brand: String::from(c.brand_str()),
            family: c.family,
            model: c.model,
            cores: c.cpu_count,
        })
    }

    fn drives(&self) -> Result<Vec<DriveInfo>, CfgError> {
        let fd = syscall::sys_ob_open("\\Global\\Info\\Drives", ob_access::READ)?;
        // 26 volumes × size_of::<DriveInfo>() (58 bytes).
        let mut buf = Aligned([0u8; 58 * 26]);
        let n = match syscall::sys_ob_query_info(fd, ObInfoClass::Drives, &mut buf.0) {
            Ok(n) => n,
            Err(e) => {
                let _ = syscall::sys_close(fd);
                return Err(CfgError::from(e));
            }
        };
        let _ = syscall::sys_close(fd);

        let entry = size_of::<syscall::DriveInfo>();
        let count = n / entry;
        let slice =
            unsafe { core::slice::from_raw_parts(buf.0.as_ptr() as *const syscall::DriveInfo, count) };
        let mut out = Vec::new();
        for d in slice {
            if d.present == 0 {
                continue;
            }
            out.push(DriveInfo {
                letter: d.letter,
                fs_type: String::from(d.fs_type_str()),
                label: String::from(d.label_str()),
                total_kib: d.total_sectors.saturating_mul(512) / 1024,
            });
        }
        Ok(out)
    }

    fn process_count(&self) -> Result<u32, CfgError> {
        let fd = syscall::sys_ob_open("\\Process", ob_access::READ)?;
        let mut entries = empty_entries();
        let n = match syscall::sys_ob_enum(fd, &mut entries) {
            Ok(n) => n,
            Err(e) => {
                let _ = syscall::sys_close(fd);
                return Err(CfgError::from(e));
            }
        };
        let _ = syscall::sys_close(fd);
        Ok(n as u32)
    }

    fn services(&self) -> Result<Vec<ServiceInfo>, CfgError> {
        let fd = syscall::sys_ob_open("\\Service", ob_access::READ)
            .map_err(|_| CfgError::ModuleUnavailable)?;
        let mut entries = empty_entries();
        let n = syscall::sys_ob_enum(fd, &mut entries).unwrap_or(0);
        let _ = syscall::sys_close(fd);

        let mut out = Vec::new();
        for e in entries.iter().take(n.min(entries.len())) {
            if e.obj_type == syscall::ob_type::SERVICE {
                out.push(ServiceInfo {
                    name: String::from(e.name_str()),
                    // Status query needs the service object fd; wired with the
                    // System module (#323).
                    running: false,
                });
            }
        }
        Ok(out)
    }

    fn keyboard_layout(&self) -> Result<String, CfgError> {
        let name = keyboard::kbd_get_layout().map_err(CfgError::from)?;
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        Ok(String::from_utf8_lossy(&name[..end]).into_owned())
    }

    fn set_keyboard_layout(&self, layout: &str) -> Result<(), CfgError> {
        keyboard::kbd_set_layout(layout).map_err(CfgError::from)
    }

    fn power(&self) -> Option<&dyn PowerOps> {
        // TODO(#326): probe `\System\PowerManager` once the plan API is in
        // libneodos. Shutdown/reboot already exist (classes 37/38).
        None
    }

    fn locale(&self) -> Option<&dyn LocaleOps> {
        self.locale.as_ref().map(|l| l as &dyn LocaleOps)
    }

    fn certificates(&self) -> Option<&dyn CertOps> {
        Some(&self.certs)
    }
}

/// Trust store operations over `libtrust` (image bundle + user registry store).
struct NeodosCerts;

fn entry(e: libtrust::Entry, system: bool) -> CertEntry {
    CertEntry {
        fingerprint: e.fingerprint,
        subject: e.subject,
        issuer: e.issuer,
        not_before: e.not_before,
        not_after: e.not_after,
        system,
    }
}

fn rejection(e: libtrust::validate::TrustError) -> CertRejection {
    use libtrust::validate::TrustError as T;
    match e {
        T::Empty => CertRejection::Empty,
        T::Malformed | T::TooLarge => CertRejection::Malformed,
        T::NotACa => CertRejection::NotCa,
        T::KeyUsageNotCertSign => CertRejection::KeyUsage,
        T::Expired => CertRejection::Expired,
        T::NotYetValid => CertRejection::NotYetValid,
        T::Unsupported => CertRejection::Unsupported,
    }
}

/// Read a whole file through the object manager (`ReadContent`).
fn read_file(path: &str) -> Option<alloc::vec::Vec<u8>> {
    let mut ob = [0u8; 256];
    let prefix = b"\\Global\\FileSystem\\";
    if ob.len() < prefix.len() {
        return None;
    }
    ob[..prefix.len()].copy_from_slice(prefix);
    let bytes = path.as_bytes();
    if prefix.len() + bytes.len() > ob.len() {
        return None;
    }
    ob[prefix.len()..prefix.len() + bytes.len()].copy_from_slice(bytes);
    let len = prefix.len() + bytes.len();
    let p = core::str::from_utf8(&ob[..len]).ok()?;
    let fd = syscall::sys_ob_open(p, ob_access::READ).ok()?;
    let mut buf = alloc::vec![0u8; 128 * 1024];
    let n = match syscall::sys_ob_query_info(fd, ObInfoClass::ReadContent, &mut buf) {
        Ok(n) => n,
        Err(_) => {
            let _ = syscall::sys_close(fd);
            return None;
        }
    };
    let _ = syscall::sys_close(fd);
    buf.truncate(n);
    Some(buf)
}

impl CertOps for NeodosCerts {
    fn entries(&self) -> Vec<CertEntry> {
        let mut out: Vec<CertEntry> = Vec::new();
        for e in libtrust::store::system_entries() {
            out.push(entry(e, true));
        }
        for e in libtrust::store::user_entries() {
            out.push(entry(e, false));
        }
        out
    }

    fn import_file(&self, path: &str) -> Vec<CertImport> {
        let Some(bytes) = read_file(path) else {
            return alloc::vec![CertImport::Rejected(CertRejection::Io)];
        };
        let Ok(now) = libclock::system::unix_seconds() else {
            // Without a trusted clock the validity window cannot be judged.
            return alloc::vec![CertImport::Rejected(CertRejection::Unsupported)];
        };
        libtrust::store::import_all(&bytes, now)
            .into_iter()
            .map(|r| match r {
                Ok(libtrust::store::Imported::Installed(fp)) => CertImport::Installed(fp),
                Ok(libtrust::store::Imported::AlreadyUser(fp)) => CertImport::AlreadyUser(fp),
                Ok(libtrust::store::Imported::AlreadySystem(fp)) => CertImport::AlreadySystem(fp),
                Err(e) => CertImport::Rejected(rejection(e)),
            })
            .collect()
    }

    fn remove(&self, fingerprint: &str) -> Result<(), CfgError> {
        libtrust::store::remove(fingerprint).map_err(|_| CfgError::ModuleUnavailable)
    }
}

/// Locale operations over the runtime i18n subsystem.
struct NeodosLocale;

impl LocaleOps for NeodosLocale {
    fn active_locale(&self) -> Result<String, CfgError> {
        Ok(String::from(i18n::i18n_active_locale()))
    }

    fn available_locales(&self) -> Result<Vec<String>, CfgError> {
        let raw = i18n::i18n_available_locales();
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        Ok(raw.split(';').map(String::from).collect())
    }

    fn set_locale(&self, tag: &str) -> Result<(), CfgError> {
        const LOCALE_KEY: &str =
            "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Locale";
        if let Ok(fd) = syscall::sys_cm_open_key(LOCALE_KEY) {
            let _ = syscall::sys_cm_set_value(fd, "Language", syscall::REG_SZ, tag.as_bytes());
            let _ = syscall::sys_cm_flush_key(fd);
            let _ = syscall::sys_close(fd);
        }
        i18n::i18n_set_language(tag);
        i18n::i18n_reload_all();
        Ok(())
    }
}
