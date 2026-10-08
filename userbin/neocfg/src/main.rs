//! NeoCfg — Ring 3 `.NXE` glue.
//!
//! This binary contains **no module logic**: it only implements the three
//! injectable seams of `libneocfg` on top of `libneodos`:
//!
//! * [`neodos_i18n::NeodosTranslator`] — `Translator`
//! * [`neodos_platform::NeodosPlatform`] — `CfgPlatform`
//! * [`tui::TuiUi`] — `CfgUi` over `libneotui`
//!
//! Entry point: `_start` → `App::run(...)`.

#![no_std]
#![no_main]

extern crate alloc;

mod neodos_i18n;
mod neodos_platform;
mod tui;

use core::alloc::{GlobalAlloc, Layout};

use libneocfg::App;
use libneodos::{i18n, mem, syscall};

/// NXE application name (NLT catalog + resource lookup).
const APP_NAME: &str = "neocfg";

/// `sbrk`-backed allocator (same pattern as the other userland tools).
struct SbrkAlloc;

unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() {
            core::ptr::null_mut()
        } else {
            ptr
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    // Fall back to the key placeholder if the catalog is missing.
    let _ = i18n::i18n_load(APP_NAME);

    let translator = neodos_i18n::NeodosTranslator;
    let platform = neodos_platform::NeodosPlatform::new();
    let mut ui = tui::TuiUi::new(translator);
    {
        let mut app = App::new(libneocfg::MODULES, &translator, &mut ui);
        let _ = app.run(&platform);
    }

    syscall::sys_exit(0);
}
