use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::object::{self, ObType};
use crate::log::LogSubsys;

mod layout;
mod unicode;
pub(crate) mod config;
pub(crate) mod event;
mod hotkey;

pub use layout::KbdLayout;

pub const KBD_SHIFT: u8      = 0x01;
pub const KBD_CTRL: u8       = 0x02;
pub const KBD_ALT: u8        = 0x04;
pub const KBD_ALTGR: u8      = 0x08;
pub const KBD_CAPS: u8       = 0x10;
pub const KBD_NUMLOCK: u8    = 0x20;
pub const KBD_SCROLLLOCK: u8 = 0x40;

pub const KBD_LED_CAPS: u8    = 0x04;
pub const KBD_LED_NUM: u8     = 0x02;
pub const KBD_LED_SCROLL: u8  = 0x01;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KbdState {
    pub modifiers: u8,
    pub leds: u8,
    pub active_layout_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KbdCaps {
    pub max_layouts: u32,
    pub supports_repeat_config: bool,
    pub supports_led_control: bool,
    pub supports_hotkeys: bool,
    pub num_layouts: u32,
    pub _pad: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct KbdLayoutInfo {
    pub index: u32,
    pub name: [u8; 32],
    pub lang_tag: [u8; 16],
    pub scancode_count: u32,
    pub compose_count: u32,
}

#[derive(Clone, Debug)]
pub struct KbdConfig {
    pub layout_name: String,
    pub repeat_delay: u32,
    pub repeat_rate: u32,
    pub numlock_on_boot: bool,
    pub capslock_on_boot: bool,
}

impl Default for KbdConfig {
    fn default() -> Self {
        KbdConfig {
            layout_name: "US".to_string(),
            repeat_delay: 500,
            repeat_rate: 30,
            numlock_on_boot: true,
            capslock_on_boot: false,
        }
    }
}

pub struct NeoKbd {
    pub state: KbdState,
    pub config: KbdConfig,
    pub layouts: Vec<KbdLayout>,
    pub dead_key: Option<u16>,
    pub ob_id: u64,
}

impl NeoKbd {
    pub fn new() -> Self {
        NeoKbd {
            state: KbdState {
                modifiers: 0,
                leds: 0,
                active_layout_index: 0,
            },
            config: KbdConfig::default(),
            layouts: Vec::new(),
            dead_key: None,
            ob_id: 0,
        }
    }

    pub fn active_layout(&self) -> Option<&KbdLayout> {
        let idx = self.state.active_layout_index as usize;
        self.layouts.get(idx)
    }

    pub fn active_layout_mut(&mut self) -> Option<&mut KbdLayout> {
        let idx = self.state.active_layout_index as usize;
        self.layouts.get_mut(idx)
    }

    pub fn find_layout_index(&self, name: &str) -> Option<u32> {
        self.layouts.iter().position(|l| {
            let layout_name = l.name_str();
            layout_name.eq_ignore_ascii_case(name)
        }).map(|i| i as u32)
    }

    pub fn set_layout_by_name(&mut self, name: &str) -> Result<(), ()> {
        let idx = self.find_layout_index(name).ok_or(())?;
        self.state.active_layout_index = idx;
        self.config.layout_name = name.to_string();
        let _ = config::kbd_save_config(&self.config);
        let _ = crate::eventbus::EVENT_BUS.push_event(
            crate::eventbus::EVENT_KEYB_LAYOUT,
            crate::eventbus::SOURCE_KERNEL,
            3, idx as u64, 0, 0,
        );
        Ok(())
    }

    pub fn set_repeat_delay(&mut self, delay_ms: u32) -> Result<(), ()> {
        if delay_ms < 100 || delay_ms > 2000 {
            return Err(());
        }
        self.config.repeat_delay = delay_ms;
        let _ = config::kbd_save_config(&self.config);
        Ok(())
    }

    pub fn set_repeat_rate(&mut self, rate_cps: u32) -> Result<(), ()> {
        if rate_cps < 2 || rate_cps > 60 {
            return Err(());
        }
        self.config.repeat_rate = rate_cps;
        let _ = config::kbd_save_config(&self.config);
        Ok(())
    }

    pub fn set_leds(&mut self, leds: u8) {
        self.state.leds = leds & 0x07;
        crate::drivers::ps2::set_leds(leds);
    }

    pub fn set_modifiers(&mut self, mods: u8) {
        let old = self.state.modifiers;
        self.state.modifiers = mods;
        let _ = crate::eventbus::EVENT_BUS.push_event(
            crate::eventbus::EVENT_KBD_MODIFIER,
            crate::eventbus::SOURCE_KERNEL,
            3, mods as u64, old as u64, 0,
        );
    }

    pub fn process_scancode(&mut self, scancode: u8, is_make: bool) {
        let code = scancode & 0x7F;
        let is_extended = scancode == 0xE0;
        if is_extended {
            return;
        }

        // Update modifiers
        match code {
            0x2A | 0x36 => {
                if is_make { self.state.modifiers |= KBD_SHIFT; }
                else { self.state.modifiers &= !KBD_SHIFT; }
            }
            0x1D => {
                if is_make { self.state.modifiers |= KBD_CTRL; }
                else { self.state.modifiers &= !KBD_CTRL; }
            }
            0x38 => {
                if is_make { self.state.modifiers |= KBD_ALT; }
                else { self.state.modifiers &= !KBD_ALT; }
            }
            0x3A => {
                if is_make {
                    self.state.modifiers ^= KBD_CAPS;
                    let leds = if (self.state.modifiers & KBD_CAPS) != 0 { KBD_LED_CAPS } else { 0 };
                    self.set_leds((self.state.leds & !KBD_LED_CAPS) | leds);
                }
            }
            0x45 => {
                if is_make {
                    self.state.modifiers ^= KBD_NUMLOCK;
                    let leds = if (self.state.modifiers & KBD_NUMLOCK) != 0 { KBD_LED_NUM } else { 0 };
                    self.set_leds((self.state.leds & !KBD_LED_NUM) | leds);
                }
            }
            0x46 => {
                if is_make {
                    self.state.modifiers ^= KBD_SCROLLLOCK;
                    let leds = if (self.state.modifiers & KBD_SCROLLLOCK) != 0 { KBD_LED_SCROLL } else { 0 };
                    self.set_leds((self.state.leds & !KBD_LED_SCROLL) | leds);
                }
            }
            _ => {}
        }

        let _ = crate::eventbus::EVENT_BUS.push_event(
            if is_make { crate::eventbus::EVENT_KEYDOWN } else { crate::eventbus::EVENT_KEYUP },
            crate::eventbus::SOURCE_KERNEL,
            3, scancode as u64, self.state.modifiers as u64, 0,
        );

        if !is_make {
            return;
        }

        if hotkey::dispatch_hotkey(code, self.state.modifiers) {
            return;
        }

        let mods = self.state.modifiers;
        if let Some(layout) = self.active_layout() {
            if let Some(codepoint) = layout::lookup_codepoint(layout, code, mods) {
                if layout::is_dead_key(layout, code, mods) {
                    self.dead_key = Some(codepoint);
                    return;
                }

                let final_cp = if let Some(dk) = self.dead_key {
                    self.dead_key = None;
                    layout::compose(dk, codepoint)
                } else {
                    codepoint
                };

                let utf8 = unicode::unicode_to_utf8(final_cp);
                for &b in utf8.iter() {
                    if b == 0 { break; }
                    let _ = crate::input::push_byte(b);
                }
                crate::syscall::wake_blocked_readers();

                let _ = crate::eventbus::EVENT_BUS.push_event(
                    crate::eventbus::EVENT_KEY_CHAR,
                    crate::eventbus::SOURCE_KERNEL,
                    3, final_cp as u64, code as u64, 0,
                );
            }
        }
    }
}

lazy_static! {
    pub static ref KBD: Mutex<NeoKbd> = Mutex::new(NeoKbd::new());
}

pub fn kbd_init() {
    kinfo!(LogSubsys::Kbd, "Initializing Keyboard Manager (NeoKBD)...");

    let _ = object::namespace::ob_create_directory("\\Device");
    if let Ok(kbd_id) = object::ob_create_object(ObType::KeyboardDevice, "Keyboard", 0, 0, None) {
        let _ = object::namespace::ob_insert_object("\\Device\\Keyboard", kbd_id);
        let mut kbd = KBD.lock();
        kbd.ob_id = kbd_id;
    }

    let count = layout::load_layouts();
    kinfo!(LogSubsys::Kbd, "Loaded {} keyboard layout(s)", count);

    let config = config::kbd_load_config();
    let mut kbd = KBD.lock();
    kbd.config = config;
    if let Some(idx) = kbd.find_layout_index(&kbd.config.layout_name) {
        kbd.state.active_layout_index = idx;
    } else if !kbd.layouts.is_empty() {
        kbd.state.active_layout_index = 0;
        kbd.config.layout_name = kbd.layouts[0].name_str().to_string();
    }
    if kbd.config.numlock_on_boot {
        kbd.state.modifiers |= KBD_NUMLOCK;
        kbd.state.leds |= KBD_LED_NUM;
    }
    if kbd.config.capslock_on_boot {
        kbd.state.modifiers |= KBD_CAPS;
        kbd.state.leds |= KBD_LED_CAPS;
    }
    let leds = kbd.state.leds;
    kbd.set_leds(leds);
    let layout_name = kbd.config.layout_name.clone();
    drop(kbd);

    event::register_kbd_event_handler();

    kinfo!(LogSubsys::Kbd, "Ready (layout: {})", layout_name);
}
