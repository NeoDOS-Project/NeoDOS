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

// ── Group SIDs predefinidos ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub sid: Sid,
    pub is_admin: bool,
    pub groups: Vec<Sid>,
    pub privileges: u64,
    pub session_id: u32,
}

impl Token {
    pub fn new(sid: Sid, is_admin: bool) -> Self {
        Token {
            sid,
            is_admin,
            groups: Vec::new(),
            privileges: if is_admin { SE_ADMIN_PRIVILEGES } else { SE_USER_PRIVILEGES },
            session_id: 0,
        }
    }

    pub fn new_full(sid: Sid, is_admin: bool, groups: Vec<Sid>, privileges: u64, session_id: u32) -> Self {
        Token { sid, is_admin, groups, privileges, session_id }
    }

    pub fn new_admin() -> Self {
        Token {
            sid: sid_builtin_admin(),
            is_admin: true,
            groups: Vec::new(),
            privileges: SE_ADMIN_PRIVILEGES,
            session_id: 0,
        }
    }

    pub fn new_user() -> Self {
        Token {
            sid: sid_builtin_user(),
            is_admin: false,
            groups: Vec::new(),
            privileges: SE_USER_PRIVILEGES,
            session_id: 1,
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

    pub fn inherit_from(parent: &Token) -> Self {
        Token {
            sid: parent.sid,
            is_admin: parent.is_admin,
            groups: parent.groups.clone(),
            privileges: parent.privileges,
            session_id: parent.session_id,
        }
    }
}
