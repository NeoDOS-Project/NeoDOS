use crate::syscall;
use crate::syscall::{ObInfoClass, ObSetInfoClass, ob_access};

pub const KBD_SHIFT: u8      = 0x01;
pub const KBD_CTRL: u8       = 0x02;
pub const KBD_ALT: u8        = 0x04;
pub const KBD_ALTGR: u8      = 0x08;
pub const KBD_CAPS: u8       = 0x10;
pub const KBD_NUMLOCK: u8    = 0x20;
pub const KBD_SCROLLLOCK: u8 = 0x40;

const KEYBOARD_DEVICE_PATH: &str = "\\Device\\Keyboard";
const LEGACY_KEYBOARD_PATH: &str = "\\Global\\Info\\Keyboard";

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KbdState {
    pub modifiers: u8,
    pub leds: u8,
    pub active_layout_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KbdLayoutInfo {
    pub index: u32,
    pub name: [u8; 32],
    pub lang_tag: [u8; 16],
    pub scancode_count: u32,
    pub compose_count: u32,
}

impl KbdLayoutInfo {
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&self.name[..end]).unwrap_or("?")
    }

    pub fn lang_tag_str(&self) -> &str {
        let end = self.lang_tag.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.lang_tag[..end]).unwrap_or("?")
    }
}

fn open_kbd(access: u32) -> Result<u8, i64> {
    match syscall::sys_ob_open(KEYBOARD_DEVICE_PATH, access) {
        Ok(fd) => Ok(fd),
        Err(_) => {
            syscall::sys_ob_open(LEGACY_KEYBOARD_PATH, access)
        }
    }
}

pub fn kbd_get_layout() -> Result<[u8; 32], i64> {
    let fd = open_kbd(ob_access::READ)?;
    let mut state = KbdState {
        modifiers: 0, leds: 0, active_layout_index: 0,
    };
    let state_buf = unsafe {
        core::slice::from_raw_parts_mut(
            &mut state as *mut KbdState as *mut u8,
            core::mem::size_of::<KbdState>(),
        )
    };
    let r = syscall::sys_ob_query_info(fd, ObInfoClass::KeyboardInfo, state_buf);
    let _ = syscall::sys_close(fd);
    match r {
        Ok(_) => {
            // Query the layout info for the active layout index
            let mut caps_buf = [0u8; 20];
            let fd2 = open_kbd(ob_access::READ)?;
            let _ = syscall::sys_ob_query_info(fd2, ObInfoClass::KeyboardCaps, &mut caps_buf);
            let _num_layouts = u32::from_le_bytes(caps_buf[16..20].try_into().unwrap()) as usize;
            let entry_size = core::mem::size_of::<KbdLayoutInfo>();
            let mut buf = [0u8; 256]; // Enough for many layouts
            let n = syscall::sys_ob_query_info(fd2, ObInfoClass::KeyboardLayouts, &mut buf).unwrap_or(0);
            let _ = syscall::sys_close(fd2);

            let count = n / entry_size;
            let idx = state.active_layout_index as usize;
            for i in 0..count {
                let offset = i * entry_size;
                if offset + entry_size > buf.len() { break; }
                let mut info = KbdLayoutInfo {
                    index: 0, name: [0u8; 32], lang_tag: [0u8; 16],
                    scancode_count: 0, compose_count: 0,
                };
                let src = &buf[offset..offset + entry_size];
                unsafe {
                    core::ptr::copy_nonoverlapping(src.as_ptr(), &mut info as *mut KbdLayoutInfo as *mut u8, entry_size);
                }
                if info.index == idx as u32 {
                    return Ok(info.name);
                }
            }
            let mut n = [0u8; 32];
            n[..5].copy_from_slice(b"Error");
            Ok(n)
        }
        Err(e) => Err(e),
    }
}

pub fn kbd_set_layout(name: &str) -> Result<(), i64> {
    let fd = open_kbd(ob_access::WRITE)?;
    let mut name_buf = [0u8; 32];
    let bytes = name.as_bytes();
    let len = bytes.len().min(31);
    name_buf[..len].copy_from_slice(&bytes[..len]);
    let r = syscall::sys_ob_set_info(fd, ObSetInfoClass::KeyboardSetLayout, &name_buf[..len+1]);
    let _ = syscall::sys_close(fd);
    r
}

pub fn kbd_list_layouts() -> Result<LayoutList, i64> {
    let fd = open_kbd(ob_access::READ)?;

    let entry_size = core::mem::size_of::<KbdLayoutInfo>();
    let mut buf = [0u8; 1024]; // Enough for ~42 layouts
    let n = syscall::sys_ob_query_info(fd, ObInfoClass::KeyboardLayouts, &mut buf)?;
    let _ = syscall::sys_close(fd);

    let count = n / entry_size;
    let max_count = count.min(64); // Safety limit
    let mut list = LayoutList { count: 0, entries: [KbdLayoutInfo {
        index: 0, name: [0u8; 32], lang_tag: [0u8; 16],
        scancode_count: 0, compose_count: 0,
    }; 64] };

    for i in 0..max_count {
        let offset = i * entry_size;
        if offset + entry_size > buf.len() { break; }
        let mut info = KbdLayoutInfo {
            index: 0, name: [0u8; 32], lang_tag: [0u8; 16],
            scancode_count: 0, compose_count: 0,
        };
        let src = &buf[offset..offset + entry_size];
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), &mut info as *mut KbdLayoutInfo as *mut u8, entry_size);
        }
        list.entries[i] = info;
        list.count = i as u32 + 1;
    }

    Ok(list)
}

pub struct LayoutList {
    pub count: u32,
    pub entries: [KbdLayoutInfo; 64],
}

impl LayoutList {
    pub fn len(&self) -> usize {
        self.count as usize
    }

    pub fn iter(&self) -> LayoutListIter<'_> {
        LayoutListIter { list: self, index: 0 }
    }
}

impl<'a> IntoIterator for &'a LayoutList {
    type Item = &'a KbdLayoutInfo;
    type IntoIter = LayoutListIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct LayoutListIter<'a> {
    list: &'a LayoutList,
    index: usize,
}

impl<'a> Iterator for LayoutListIter<'a> {
    type Item = &'a KbdLayoutInfo;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.list.count as usize {
            let item = &self.list.entries[self.index];
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }
}

pub fn kbd_get_repeat() -> Result<(u32, u32), i64> {
    let reg_fd = syscall::sys_cm_open_key("\\Registry\\Machine\\System\\Keyboard")?;
    let delay = read_reg_dword(reg_fd, "RepeatDelay").unwrap_or(500);
    let rate = read_reg_dword(reg_fd, "RepeatRate").unwrap_or(30);
    let _ = syscall::sys_close(reg_fd);
    Ok((delay, rate))
}

pub fn kbd_set_repeat(delay_ms: u32, rate_cps: u32) -> Result<(), i64> {
    let fd = open_kbd(ob_access::WRITE)?;
    let r1 = syscall::sys_ob_set_info(fd, ObSetInfoClass::KeyboardSetRepeatDelay, &delay_ms.to_le_bytes());
    let r2 = syscall::sys_ob_set_info(fd, ObSetInfoClass::KeyboardSetRepeatRate, &rate_cps.to_le_bytes());
    let _ = syscall::sys_close(fd);
    r1.and(r2)
}

pub fn kbd_get_state() -> Result<KbdState, i64> {
    let fd = open_kbd(ob_access::READ)?;
    let mut state = KbdState {
        modifiers: 0, leds: 0, active_layout_index: 0,
    };
    let state_buf = unsafe {
        core::slice::from_raw_parts_mut(
            &mut state as *mut KbdState as *mut u8,
            core::mem::size_of::<KbdState>(),
        )
    };
    let r = syscall::sys_ob_query_info(fd, ObInfoClass::KeyboardInfo, state_buf);
    let _ = syscall::sys_close(fd);
    r.map(|_| state)
}

pub fn kbd_set_leds(leds: u8) -> Result<(), i64> {
    let fd = open_kbd(ob_access::WRITE)?;
    let r = syscall::sys_ob_set_info(fd, ObSetInfoClass::KeyboardSetLeds, &[leds]);
    let _ = syscall::sys_close(fd);
    r
}

fn read_reg_dword(fd: u8, name: &str) -> Option<u32> {
    let mut buf = [0u8; 12];
    match syscall::sys_cm_query_value(fd, name, &mut buf) {
        Ok(n) if n >= 12 => {
            let data_type = u32::from_le_bytes(buf[0..4].try_into().unwrap());
            if data_type == 2 {
                Some(u32::from_le_bytes(buf[8..12].try_into().unwrap()))
            } else {
                None
            }
        }
        _ => None,
    }
}
