//! On-disk layout paths (#398).
//!
//! Mirrors `neodos-kernel/src/paths.rs`: userland code should name system
//! locations through these constants instead of hardcoding the tree, so the
//! layout has a single source of truth per side of the ABI.

/// System root on the system volume (`.NXE` de sistema).
pub const SYSTEM_ROOT: &str = "C:\\NeoDOS\\System";
/// Shared libraries (`.NXL`).
pub const LIBRARIES_DIR: &str = "C:\\NeoDOS\\Libraries";
/// Localization data (`*.nlt`).
pub const GLOBALIZATION_DIR: &str = "C:\\NeoDOS\\System\\Globalization";
/// Keyboard layouts (`.kbd`).
pub const KEYBOARD_DIR: &str = "C:\\NeoDOS\\System\\Keyboard";
/// Registry hives.
pub const CONFIG_DIR: &str = "C:\\NeoDOS\\System\\config";
/// User profiles.
pub const USERS_ROOT: &str = "C:\\Users";
