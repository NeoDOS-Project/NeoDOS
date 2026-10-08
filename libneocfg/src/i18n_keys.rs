//! Numeric i18n key constants for NeoCfg.
//!
//! Must stay in sync with the `[ids]` tables in
//! `data/locale/{en-US,es-ES,ca-ES}/neocfg.toml`.
//!
//! These can be regenerated from the TOML sources with:
//!
//! ```text
//! nltc --generate-rust data/locale/en-US/neocfg.toml src/i18n_keys.rs
//! ```

// ── Navigation ─────────────────────────────────────────────────────────
pub const NEOCFG_TITLE: u32 = 1001;
pub const NEOCFG_EXIT: u32 = 1002;
pub const NEOCFG_BACK: u32 = 1003;
pub const NEOCFG_SELECT_HINT: u32 = 1004;

// ── Module names / descriptions ────────────────────────────────────────
pub const MODULE_SYSTEM_NAME: u32 = 1005;
pub const MODULE_SYSTEM_DESC: u32 = 1006;
pub const MODULE_POWER_NAME: u32 = 1007;
pub const MODULE_POWER_DESC: u32 = 1008;
pub const MODULE_LOCALE_NAME: u32 = 1009;
pub const MODULE_LOCALE_DESC: u32 = 1010;
pub const MODULE_KEYBOARD_NAME: u32 = 1011;
pub const MODULE_KEYBOARD_DESC: u32 = 1012;
pub const MODULE_ABOUT_NAME: u32 = 1013;
pub const MODULE_ABOUT_DESC: u32 = 1014;

// ── Misc ───────────────────────────────────────────────────────────────
pub const MODULE_PENDING: u32 = 1015;
pub const POWER_NOT_AVAILABLE: u32 = 1016;
pub const LOCALE_NOT_AVAILABLE: u32 = 1017;
pub const PRESS_KEY: u32 = 1018;
pub const NEOCFG_YES: u32 = 1019;
pub const NEOCFG_NO: u32 = 1020;
