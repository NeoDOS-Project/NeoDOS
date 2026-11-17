#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

mod commands;
mod output;
mod parse;

use libneodos::i18n;
use libneodos::syscall;

const APP_NAME: &str = "neokey";

// ── Locale IDs (keep in sync with data/locale/*/neokey.toml) ──
// NOTE: kept in `main.rs` because `scripts/check-i18n.sh` verifies the
// code<->catalog bindings against this file only.
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_USAGE_LINE3: u32 = 1003;
const IDS_USAGE_LINE4: u32 = 1004;
const IDS_KBD_STATE: u32 = 1005;
const IDS_MODIFIERS: u32 = 1006;
const IDS_LEDS: u32 = 1007;
const IDS_ACTIVE_LAYOUT: u32 = 1008;
const IDS_AVAIL_LAYOUTS: u32 = 1009;
const IDS_UNKNOWN_CMD: u32 = 1010;
const IDS_LAYOUT_SET: u32 = 1011;
const IDS_ERR_SET_LAYOUT: u32 = 1012;
const IDS_MISSING_LAYOUT: u32 = 1013;
const IDS_USAGE_LINE5: u32 = 1014;
const IDS_USAGE_LINE6: u32 = 1015;
const IDS_USAGE_LINE7: u32 = 1016;
const IDS_USAGE_LINE8: u32 = 1017;
const IDS_REPEAT: u32 = 1018;
const IDS_DELAY: u32 = 1019;
const IDS_REPEAT_SET: u32 = 1020;
const IDS_DELAY_SET: u32 = 1021;
const IDS_LEDS_SET: u32 = 1022;
const IDS_ERR_SET: u32 = 1023;
const IDS_INVALID_VALUE: u32 = 1024;
const IDS_CPS: u32 = 1025;
const IDS_MS: u32 = 1026;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        commands::print_help();
        syscall::sys_exit(0);
    }

    let args = libneodos::args::trim_ascii(&raw);
    if args.is_empty() {
        syscall::sys_exit(commands::show_state());
    }

    let arg_str = core::str::from_utf8(args).unwrap_or("");
    let mut words = arg_str.split_whitespace();
    let cmd = words.next().unwrap_or("");
    let value = words.next();

    let code = if cmd.eq_ignore_ascii_case("show") {
        commands::show_state()
    } else if cmd.eq_ignore_ascii_case("layouts")
        || cmd.eq_ignore_ascii_case("--list")
        || cmd.eq_ignore_ascii_case("-l")
    {
        commands::list_layouts()
    } else if cmd.eq_ignore_ascii_case("layout") {
        match value {
            Some(name) if !name.is_empty() => commands::set_layout(name),
            _ => commands::missing_layout(),
        }
    } else if cmd.eq_ignore_ascii_case("repeat") {
        match value.and_then(parse::u32) {
            Some(cps) => commands::set_repeat_rate(cps),
            None => commands::invalid_value(),
        }
    } else if cmd.eq_ignore_ascii_case("delay") {
        match value.and_then(parse::u32) {
            Some(ms) => commands::set_repeat_delay(ms),
            None => commands::invalid_value(),
        }
    } else if cmd.eq_ignore_ascii_case("leds") {
        match value.and_then(parse::led_mask) {
            Some(bits) => commands::set_leds(bits),
            None => commands::invalid_value(),
        }
    } else {
        commands::unknown_command()
    };

    syscall::sys_exit(code)
}
