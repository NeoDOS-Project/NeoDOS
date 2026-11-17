use crate::security::sid::{Sid, sid_builtin_admin, sid_builtin_user};
use alloc::vec::Vec;

// ── Privilege flags ─────────────────────────────────────────────────

pub const SE_CREATE_TOKEN_PRIVILEGE: u64 = 1 << 0;
pub const SE_TCB_PRIVILEGE: u64 = 1 << 1;
pub const SE_LOAD_DRIVER_PRIVILEGE: u64 = 1 << 2;
pub const SE_SHUTDOWN_PRIVILEGE: u64 = 1 << 3;
pub const SE_DEBUG_PRIVILEGE: u64 = 1 << 4;
pub const SE_SYSTEM_ENVIRONMENT_PRIVILEGE: u64 = 1 << 5;
pub const SE_CHANGE_NOTIFY_PRIVILEGE: u64 = 1 << 6;
pub const SE_BACKUP_PRIVILEGE: u64 = 1 << 7;
pub const SE_RESTORE_PRIVILEGE: u64 = 1 << 8;
pub const SE_TAKE_OWNERSHIP_PRIVILEGE: u64 = 1 << 9;
pub const SE_INCREASE_QUOTA_PRIVILEGE: u64 = 1 << 10;
pub const SE_MANAGE_VOLUME_PRIVILEGE: u64 = 1 << 11;

pub const SE_ADMIN_PRIVILEGES: u64 = 0xFFFF;
pub const SE_USER_PRIVILEGES: u64 = SE_CHANGE_NOTIFY_PRIVILEGE;

// ── Integrity levels (NT Mandatory Integrity Control) ───────────────

/// NT Mandatory Integrity Control (MIC) level. Higher levels may write to
/// lower or equal levels; a lower-integrity token cannot write up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum IntegrityLevel {
    Untrusted = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    System = 4,
}

impl IntegrityLevel {
    pub fn to_str(self) -> &'static str {
        match self {
            IntegrityLevel::Untrusted => "UNTRUSTED",
            IntegrityLevel::Low => "LOW",
            IntegrityLevel::Medium => "MEDIUM",
            IntegrityLevel::High => "HIGH",
            IntegrityLevel::System => "SYSTEM",
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(IntegrityLevel::Untrusted),
            1 => Some(IntegrityLevel::Low),
            2 => Some(IntegrityLevel::Medium),
            3 => Some(IntegrityLevel::High),
            4 => Some(IntegrityLevel::System),
            _ => None,
        }
    }
}

/// Monotonic system timestamp (TSC ticks) used for `Token::creation_time`.
fn token_timestamp() -> u64 {
    // SAFETY: `rdtsc` is a baseline, side-effect-free x86_64 instruction; the
    // kernel always runs on x86_64.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

// ── Token ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub sid: Sid,
    pub is_admin: bool,
    pub groups: Vec<Sid>,
    pub privileges: u64,
    pub session_id: u32,
    /// Mandatory Integrity Control level (admin = System, user = Medium).
    pub integrity_level: IntegrityLevel,
    /// System timestamp (TSC ticks) at token creation.
    pub creation_time: u64,
}

impl Token {
    /// Integrity level implied by a token's admin flag (NT convention:
    /// administrators run at System, standard users at Medium).
    fn default_integrity(is_admin: bool) -> IntegrityLevel {
        if is_admin { IntegrityLevel::System } else { IntegrityLevel::Medium }
    }

    pub fn new(sid: Sid, is_admin: bool) -> Self {
        Token {
            sid,
            is_admin,
            groups: Vec::new(),
            privileges: if is_admin { SE_ADMIN_PRIVILEGES } else { SE_USER_PRIVILEGES },
            session_id: 0,
            integrity_level: Self::default_integrity(is_admin),
            creation_time: token_timestamp(),
        }
    }

    pub fn new_full(sid: Sid, is_admin: bool, groups: Vec<Sid>, privileges: u64, session_id: u32) -> Self {
        Token {
            sid,
            is_admin,
            groups,
            privileges,
            session_id,
            integrity_level: Self::default_integrity(is_admin),
            creation_time: token_timestamp(),
        }
    }

    pub fn new_admin() -> Self {
        Token {
            sid: sid_builtin_admin(),
            is_admin: true,
            groups: Vec::new(),
            privileges: SE_ADMIN_PRIVILEGES,
            session_id: 0,
            integrity_level: IntegrityLevel::System,
            creation_time: token_timestamp(),
        }
    }

    pub fn new_user() -> Self {
        Token {
            sid: sid_builtin_user(),
            is_admin: false,
            groups: Vec::new(),
            privileges: SE_USER_PRIVILEGES,
            session_id: 1,
            integrity_level: IntegrityLevel::Medium,
            creation_time: token_timestamp(),
        }
    }

    pub fn is_admin_token(&self) -> bool {
        self.is_admin
    }

    pub fn add_group(&mut self, group_sid: Sid) {
        if !self.groups.contains(&group_sid) {
            self.groups.push(group_sid);
        }
    }

    pub fn is_in_group(&self, group_sid: &Sid) -> bool {
        self.groups.contains(group_sid)
    }

    pub fn has_privilege(&self, privilege: u64) -> bool {
        self.privileges & privilege != 0
    }

    pub fn enable_privilege(&mut self, privilege: u64) {
        self.privileges |= privilege;
    }

    pub fn disable_privilege(&mut self, privilege: u64) {
        self.privileges &= !privilege;
    }

    pub fn integrity_level(&self) -> IntegrityLevel {
        self.integrity_level
    }

    pub fn set_integrity_level(&mut self, level: IntegrityLevel) {
        self.integrity_level = level;
    }

    /// Create a child token inheriting the parent's identity, groups, privileges
    /// and integrity level, with a fresh creation timestamp.
    pub fn inherit_from(parent: &Token) -> Self {
        Token {
            sid: parent.sid,
            is_admin: parent.is_admin,
            groups: parent.groups.clone(),
            privileges: parent.privileges,
            session_id: parent.session_id,
            integrity_level: parent.integrity_level,
            creation_time: token_timestamp(),
        }
    }
}
