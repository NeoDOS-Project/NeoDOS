use crate::security::acl::{
    Acl, SecurityDescriptor, ACE_TYPE_ACCESS_ALLOWED, ACE_TYPE_ACCESS_DENIED,
    ACE_TYPE_SYSTEM_AUDIT,
};
use crate::security::token::Token;
use crate::security::sid::Sid;
use crate::log::LogSubsys;
use core::sync::atomic::{AtomicBool, Ordering};

/// Global audit switch. When enabled, `se_audit()` emits a record for every
/// matching SYSTEM_AUDIT ACE in a descriptor's SACL. Disabled by default, so
/// the common path pays only one atomic load.
static SECURITY_AUDITING: AtomicBool = AtomicBool::new(false);

pub fn set_auditing(enabled: bool) {
    SECURITY_AUDITING.store(enabled, Ordering::SeqCst);
}

pub fn auditing_enabled() -> bool {
    SECURITY_AUDITING.load(Ordering::SeqCst)
}

/// True when `ace_sid` is the token's primary SID or one of its group SIDs.
fn trustee_matches(ace_sid: &Sid, token_sid: &Sid, groups: &[Sid]) -> bool {
    ace_sid == token_sid || groups.iter().any(|g| g == ace_sid)
}

/// NT-correct DACL evaluation.
///
/// All Deny ACEs are evaluated first (in order), then all Allow ACEs. An ACE
/// matches when its trustee SID equals the token's primary SID **or any group
/// SID**. Returns `true` only when a matching Allow ACE covers the requested
/// access and no matching Deny ACE does.
///
/// An empty DACL denies all access (handled by the caller); a NULL DACL grants
/// full access (also handled by the caller).
fn check_dacl(dacl: &Acl, token_sid: &Sid, groups: &[Sid], desired_access: u32) -> bool {
    // ── Phase 1: check all Deny ACEs first ──
    for ace in &dacl.aces {
        if ace.ace_type == ACE_TYPE_ACCESS_DENIED
            && trustee_matches(&ace.sid, token_sid, groups)
            && (ace.access_mask & desired_access) == desired_access
        {
            return false;
        }
    }
    // ── Phase 2: check all Allow ACEs ──
    for ace in &dacl.aces {
        if ace.ace_type == ACE_TYPE_ACCESS_ALLOWED
            && trustee_matches(&ace.sid, token_sid, groups)
            && (ace.access_mask & desired_access) == desired_access
        {
            return true;
        }
    }
    false
}

/// Full access check against a token (primary SID + group SIDs).
///
/// Semantics (NT):
/// - Admin token → grant.
/// - Absent descriptor (`None`) → unprotected, grant.
/// - NULL DACL (`dacl == None`) → full access, grant.
/// - Empty DACL (`Some` with no ACEs) → deny all.
/// - Otherwise → deny-first evaluation over the token's SID and groups.
///
/// Also emits a SACL audit record via `se_audit()` before returning.
pub fn se_access_check(
    token: &Token,
    sd: Option<&SecurityDescriptor>,
    desired_access: u32,
) -> bool {
    let granted = se_access_check_inner(token, sd, desired_access);
    se_audit(token, sd, desired_access, granted);
    granted
}

fn se_access_check_inner(
    token: &Token,
    sd: Option<&SecurityDescriptor>,
    desired_access: u32,
) -> bool {
    if token.is_admin_token() {
        return true;
    }
    let sd = match sd {
        Some(s) => s,
        None => return true, // no descriptor → unprotected
    };
    let dacl = match &sd.dacl {
        Some(a) => a,
        None => return true, // NULL DACL → full access
    };
    if dacl.is_empty() {
        return false; // empty DACL → deny all
    }
    check_dacl(dacl, &token.sid, &token.groups, desired_access)
}

/// SID-only variant for callers without a full `Token` (no group membership).
///
/// Same NULL/empty DACL semantics as `se_access_check`; the trustee is the
/// single supplied SID.
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
        None => return true, // NULL DACL → full access
    };
    if dacl.is_empty() {
        return false; // empty DACL → deny all
    }
    check_dacl(dacl, token_sid, &[], desired_access)
}

/// Emit SACL audit records for an access attempt.
///
/// No-op unless auditing is enabled and the descriptor carries a SACL with a
/// `SYSTEM_AUDIT` ACE matching the trustee and the requested access. This is a
/// logging hook only: it never changes the access decision, and an absent SACL
/// is handled gracefully.
pub fn se_audit(
    token: &Token,
    sd: Option<&SecurityDescriptor>,
    desired_access: u32,
    granted: bool,
) {
    if !auditing_enabled() {
        return;
    }
    let sd = match sd {
        Some(s) => s,
        None => return,
    };
    let sacl = match &sd.sacl {
        Some(s) => s,
        None => return, // absent SACL is fine
    };
    for ace in &sacl.aces {
        if ace.ace_type == ACE_TYPE_SYSTEM_AUDIT
            && trustee_matches(&ace.sid, &token.sid, &token.groups)
            && (ace.access_mask & desired_access) == desired_access
        {
            kinfo!(
                LogSubsys::Security,
                "AUDIT sid={} access={:#x} granted={}",
                token.sid,
                desired_access,
                granted
            );
        }
    }
}
