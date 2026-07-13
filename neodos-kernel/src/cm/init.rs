use alloc::string::ToString;
use alloc::vec::Vec;

use crate::object::{self, ObType};
use crate::object::namespace;
use self::super::hive::Hive;
use self::super::manager::{CM_MANAGER, encode_cell};

pub fn init_cm() {
    let _ = crate::globals::with_vfs(|vfs| -> Result<(), ()> {
        if vfs.resolve_path("C:\\System\\Registry").is_err() {
            let _ = vfs.resolve_path("C:\\System").map_err(|_| ())?;
            vfs.mkdir("C:\\System\\Registry").map_err(|_| ())?;
        }
        Ok(())
    });

    let cm = CM_MANAGER.lock();

    let _ = namespace::ob_create_directory("\\Registry\\Machine");
    let _ = namespace::ob_create_directory("\\Registry\\User");
    drop(cm);

    mount_system_hive();
}

fn mount_system_hive() {
    let name = "SYSTEM";
    let mount_path = "\\Registry\\Machine\\System";

    let hive = load_hive_from_vfs(name).unwrap_or_else(|| Hive::new(name));

    let encoded = encode_cell(0, 0);
    if let Ok(ob_id) = object::ob_create_object(
        ObType::Key,
        "System",
        encoded,
        0,
        None,
    ) {
        let _ = namespace::ob_create_directory("\\Registry\\Machine\\System");
        let _ = namespace::ob_insert_object("\\Registry\\Machine\\System", ob_id);

        let mut cm = CM_MANAGER.lock();
        cm.mount(name, mount_path, hive, ob_id);
    }
}

fn load_hive_from_vfs(name: &str) -> Option<Hive> {
    let file_path = alloc::format!("C:\\System\\Registry\\{}.hiv", name);
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(&file_path).ok()?;
        let size = node.size as usize;
        if size < 16 {
            return None;
        }
        let mut buf = alloc::vec![0u8; size];
        let read = vfs.read(drive_idx, node.inode, 0, &mut buf).ok()?;
        buf.truncate(read);
        let mut hive = Hive::deserialize(&buf).ok()?;
        hive.name = name.to_string();
        Some(hive)
    })
}

pub fn flush_hive_to_vfs(hive: &Hive) -> Result<(), ()> {
    if !hive.is_dirty() {
        return Ok(());
    }
    let data = hive.serialize();
    let file_path = alloc::format!("C:\\System\\Registry\\{}.hiv", hive.name);
    crate::globals::with_vfs(|vfs| {
        let _ = vfs.remove_file(&file_path);
        let node = vfs.create(&file_path).map_err(|_| ())?;
        let (drive_idx, _) = vfs.resolve_path(&file_path).map_err(|_| ())?;
        vfs.write(drive_idx, node.inode, 0, &data).map_err(|_| ())?;
        Ok(())
    })
}

/// Ensure `Language = "en-US"` exists in the SYSTEM hive under
/// `CurrentControlSet\Control\Locale`.  Called once at boot so that
/// userspace `i18n_init()` always finds a value.
pub fn ensure_language_default() {
    let ctrl = match crate::cm::cm_open_key(0, "CurrentControlSet\\Control") {
        Ok(k) => k,
        Err(_) => return,
    };
    let locale = crate::cm::cm_open_key(ctrl, "Locale")
        .or_else(|_| crate::cm::cm_create_key(ctrl, "Locale"));
    let locale = match locale {
        Ok(k) => k,
        Err(_) => return,
    };
    if crate::cm::cm_query_value(locale, "Language").is_err() {
        let _ = crate::cm::cm_set_value(locale, "Language", crate::cm::hive::REG_SZ, b"en-US");
    }
}

pub fn ensure_key_path(hive: &mut Hive, start: u32, path: &str) -> Option<u32> {
    let parts: Vec<&str> = path.split('\\').filter(|p| !p.is_empty()).collect();
    let mut curr = start;
    for part in &parts {
        curr = match hive.find_key(curr, part) {
            Some(found) => found,
            None => hive.create_key(curr, part)?,
        };
    }
    Some(curr)
}
