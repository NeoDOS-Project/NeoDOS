use crate::security::acl::{Acl, SecurityDescriptor, ACE_TYPE_ACCESS_ALLOWED, ACE_TYPE_ACCESS_DENIED};
use crate::security::token::Token;
use crate::security::sid::Sid;

/// NT-correct access check.
///
/// In Windows NT, all Deny ACEs are evaluated first (in order of appearance),
/// then all Allow ACEs (in order of appearance). If a Deny ACE matches,
/// access is denied regardless of subsequent Allow ACEs.
fn check_dacl(dacl: &Acl, sid: &Sid, desired_access: u32) -> bool {
    // ── Phase 1: check all Deny ACEs first ──
    for ace in &dacl.aces {
        if ace.ace_type == ACE_TYPE_ACCESS_DENIED && ace.sid == *sid
            && (ace.access_mask & desired_access) == desired_access {
            return false;
        }
    }
    // ── Phase 2: check all Allow ACEs ──
    for ace in &dacl.aces {
        if ace.ace_type == ACE_TYPE_ACCESS_ALLOWED && ace.sid == *sid
            && (ace.access_mask & desired_access) == desired_access {
            return true;
        }
    }
    false
}

pub fn se_access_check(
    token: &Token,
    sd: Option<&SecurityDescriptor>,
    desired_access: u32,
) -> bool {
    if token.is_admin_token() {
        return true;
    }
    let sd = match sd {
        Some(s) => s,
        None => return true,
    };
    let dacl = match &sd.dacl {
        Some(a) => a,
        None => return true,
    };
    if dacl.is_empty() {
        return true;
    }
    check_dacl(dacl, &token.sid, desired_access)
}

pub fn se_access_check_sid(
    token_sid: &Sid,
    is_admin: bool,
    dacl: Option<&Acl>,
    desired_access: u32,
) -> bool {
    if is_admin {
        return true;
    }
    let dacl = match dacl {
        Some(a) => a,
        None => return true,
    };
    if dacl.is_empty() {
        return true;
    }
    check_dacl(dacl, token_sid, desired_access)
}
