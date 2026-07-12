use alloc::string::ToString;
use crate::kbd::KbdConfig;

const REG_PATH: &str = "\\Registry\\Machine\\System\\Keyboard";

pub fn kbd_load_config() -> KbdConfig {
    let mut config = KbdConfig::default();

    let token = crate::security::token::Token::new_admin();
    if let Ok(fd) = crate::object::ob_open_path("\\Registry\\Machine\\System\\Keyboard", &token, 1) {
        if let Some(obj) = crate::object::ob_lookup(fd) {
            let native_id = obj.native_id;
            if let Some(vc) = crate::cm::cm_query_value(native_id, "Layout").ok() {
                if let Some(s) = vc.as_str() {
                    if !s.is_empty() {
                        config.layout_name = s.to_string();
                    }
                }
            }
            if let Some(vc) = crate::cm::cm_query_value(native_id, "RepeatDelay").ok() {
                if let Some(d) = vc.as_dword() {
                    config.repeat_delay = d;
                }
            }
            if let Some(vc) = crate::cm::cm_query_value(native_id, "RepeatRate").ok() {
                if let Some(r) = vc.as_dword() {
                    config.repeat_rate = r;
                }
            }
            if let Some(vc) = crate::cm::cm_query_value(native_id, "NumLockOnBoot").ok() {
                if let Some(n) = vc.as_dword() {
                    config.numlock_on_boot = n != 0;
                }
            }
            if let Some(vc) = crate::cm::cm_query_value(native_id, "CapsLockOnBoot").ok() {
                if let Some(c) = vc.as_dword() {
                    config.capslock_on_boot = c != 0;
                }
            }
        }
        let _ = crate::object::ob_close_object(fd);
    }

    config
}

pub fn kbd_save_config(config: &KbdConfig) -> Result<(), ()> {
    let token = crate::security::token::Token::new_admin();
    if let Ok(fd) = crate::object::ob_open_path("\\Registry\\Machine\\System\\Keyboard", &token, 2) {
        if let Some(obj) = crate::object::ob_lookup(fd) {
            let native_id = obj.native_id;
            let _ = crate::cm::cm_set_value(native_id, "Layout", crate::cm::hive::REG_SZ, config.layout_name.as_bytes());
            let _ = crate::cm::cm_set_value(native_id, "RepeatDelay", crate::cm::hive::REG_DWORD, &config.repeat_delay.to_le_bytes());
            let _ = crate::cm::cm_set_value(native_id, "RepeatRate", crate::cm::hive::REG_DWORD, &config.repeat_rate.to_le_bytes());
            let _ = crate::cm::cm_set_value(native_id, "NumLockOnBoot", crate::cm::hive::REG_DWORD, &(config.numlock_on_boot as u32).to_le_bytes());
            let _ = crate::cm::cm_set_value(native_id, "CapsLockOnBoot", crate::cm::hive::REG_DWORD, &(config.capslock_on_boot as u32).to_le_bytes());
        }
        let _ = crate::object::ob_close_object(fd);
    }
    Ok(())
}
