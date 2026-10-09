pub mod sid;
pub mod token;
pub mod acl;
pub mod access;
pub mod sam;

use lazy_static::lazy_static;
use crate::log::LogSubsys;

lazy_static! {
    pub static ref DEFAULT_ADMIN_TOKEN: token::Token = token::Token::new_admin();
    pub static ref DEFAULT_USER_TOKEN: token::Token = token::Token::new_user();

    /// Global SAM database, seeded with the built-in accounts on first use
    /// (Administrator, Guest, SYSTEM). See `docs/design/users-security-design.md`.
    pub static ref SAM_DB: spin::Mutex<sam::SamDatabase> =
        spin::Mutex::new(sam::SamDatabase::with_builtins());
}

pub fn init_security() {
    kinfo!(LogSubsys::Security, "Security subsystem initialized");
    kinfo!(LogSubsys::Security, "System SID: {}", sid::sid_builtin_system());
    kinfo!(LogSubsys::Security, "Administrator SID: {}", sid::sid_builtin_administrator());
    kinfo!(LogSubsys::Security, "Guest SID: {}", sid::sid_builtin_guest());
    kinfo!(LogSubsys::Security, "User SID: {}", sid::sid_builtin_user());
    let count = SAM_DB.lock().entry_count();
    kinfo!(LogSubsys::Security, "SAM initialized with {} built-in accounts", count);
}

pub fn register_security_tests() {
    sam::register_sam_tests();
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;
    use crate::test_true;
    use crate::security::sid::*;
    use crate::security::token::*;
    use crate::security::acl::*;
    use crate::security::access::*;

    // ── NT6.1 Tests ─────────────────────────────────────────────────

    test_case!("sid_format", {
        let admin_sid = sid_builtin_admin();
        let s = admin_sid.format_string();
        test_true!(!s.is_empty());
        test_true!(s.starts_with("S-"));
        test_eq!(admin_sid.revision, 1);
        test_eq!(admin_sid.sub_authority_count, 1);
        test_eq!(admin_sid.sub_authorities[0], 18);
    });

    test_case!("sid_well_known_builtins", {
        test_eq!(sid_builtin_system().format_string(), "S-1-5-18");
        test_eq!(sid_builtin_administrator().format_string(), "S-1-5-21-0-0-0-500");
        test_eq!(sid_builtin_guest().format_string(), "S-1-5-21-0-0-0-501");
        test_eq!(sid_builtin_user().format_string(), "S-1-5-21-0-0-0-1000");
    });

    test_case!("sid_well_known_table", {
        test_eq!(well_known_sid("Administrator").unwrap(), sid_builtin_administrator());
        test_eq!(well_known_sid("guest").unwrap(), sid_builtin_guest());
        test_eq!(well_known_sid("SYSTEM").unwrap(), sid_builtin_system());
        test_true!(well_known_sid("Nobody").is_none());
    });

    test_case!("sam_global_seeded", {
        let count = SAM_DB.lock().entry_count();
        test_true!(count >= 3);
        test_true!(SAM_DB.lock().find_by_username("Administrator").is_some());
        test_true!(SAM_DB.lock().find_by_sid(&sid_builtin_guest()).is_some());
    });

    test_case!("token_admin_boot_default", {
        let admin_token = Token::new_admin();
        test_true!(admin_token.is_admin_token());
        test_eq!(admin_token.sid, sid_builtin_admin());

        let user_token = Token::new_user();
        test_true!(!user_token.is_admin_token());
        test_eq!(user_token.sid, sid_builtin_user());
    });

    test_case!("token_integrity_defaults", {
        // USR-P1b: admin = System IL, standard user = Medium IL.
        test_eq!(Token::new_admin().integrity_level, IntegrityLevel::System);
        test_eq!(Token::new_user().integrity_level, IntegrityLevel::Medium);
        test_eq!(Token::new(sid_builtin_admin(), true).integrity_level, IntegrityLevel::System);
        test_eq!(Token::new(sid_builtin_user(), false).integrity_level, IntegrityLevel::Medium);

        test_eq!(IntegrityLevel::System.to_u8(), 4);
        test_eq!(IntegrityLevel::Medium.to_u8(), 2);
        test_eq!(IntegrityLevel::Medium.to_str(), "MEDIUM");
        test_eq!(IntegrityLevel::from_u8(1), Some(IntegrityLevel::Low));
        test_true!(IntegrityLevel::from_u8(9).is_none());
        test_true!(IntegrityLevel::System > IntegrityLevel::Medium);
    });

    test_case!("token_creation_time_and_inherit", {
        // creation_time is a monotonic timestamp; inheritance keeps the IL.
        let parent = Token::new_user();
        let child = Token::inherit_from(&parent);
        test_true!(child.creation_time >= parent.creation_time);
        test_eq!(child.integrity_level, parent.integrity_level);
        test_eq!(child.sid, parent.sid);
        test_eq!(child.groups, parent.groups);

        let admin = Token::new_admin();
        let admin_child = Token::inherit_from(&admin);
        test_eq!(admin_child.integrity_level, IntegrityLevel::System);

        let mut t = Token::new_user();
        t.set_integrity_level(IntegrityLevel::Low);
        test_eq!(t.integrity_level(), IntegrityLevel::Low);
    });

    test_case!("token_inherit", {
        let parent = Token::new_admin();
        let child = Token::new(parent.sid, parent.is_admin);
        test_true!(child.is_admin_token());
        test_eq!(child.sid, parent.sid);
        test_eq!(child.sid, sid_builtin_admin());

        let user_parent = Token::new_user();
        let user_child = Token::new(user_parent.sid, user_parent.is_admin);
        test_true!(!user_child.is_admin_token());
        test_eq!(user_child.sid, user_parent.sid);
        test_eq!(user_child.sid, sid_builtin_user());
    });

    // ── NT6.2 Tests ─────────────────────────────────────────────────

    test_case!("acl_allow_access", {
        let user = sid_builtin_user();
        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(user, ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        let token = Token::new_user();
        test_true!(se_access_check(&token, Some(&sd), ACCESS_READ));
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_WRITE));
    });

    test_case!("acl_deny_access", {
        let user = sid_builtin_user();
        let admin_s = sid_builtin_admin();
        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(admin_s, ACCESS_ALL));
        acl.add_ace(Ace::deny(user, ACCESS_ALL));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        let token = Token::new_user();
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_READ));
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_WRITE));

        let admin_token = Token::new_admin();
        test_true!(se_access_check(&admin_token, Some(&sd), ACCESS_ALL));
    });

    test_case!("acl_inherit_parent", {
        let parent_sd = SecurityDescriptor::new();
        let child_sd = parent_sd.clone();
        test_eq!(child_sd.revision, parent_sd.revision);
        test_true!(child_sd.dacl.is_none());
        test_true!(child_sd.owner.is_none());
    });

    // ── NT6.3 Tests ─────────────────────────────────────────────────

    test_case!("se_access_check_deny", {
        let user = sid_builtin_user();
        let mut acl = Acl::new();
        acl.add_ace(Ace::deny(user, ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        let token = Token::new_user();
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_READ));
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_ALL));
    });

    test_case!("se_access_check_allow", {
        let user = sid_builtin_user();
        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(user, ACCESS_READ | ACCESS_WRITE));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        let token = Token::new_user();
        test_true!(se_access_check(&token, Some(&sd), ACCESS_READ));
        test_true!(se_access_check(&token, Some(&sd), ACCESS_WRITE));
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_EXECUTE));
    });

    test_case!("se_access_check_admin_override", {
        let user = sid_builtin_user();
        let mut acl = Acl::new();
        acl.add_ace(Ace::deny(user, ACCESS_ALL));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        let admin_token = Token::new_admin();
        test_true!(se_access_check(&admin_token, Some(&sd), ACCESS_ALL));
        test_true!(se_access_check(&admin_token, Some(&sd), ACCESS_READ));
    });

    // ── NT6.4 Tests ─────────────────────────────────────────────────

    test_case!("se_admin_required", {
        let (tx, _rx) = {
            let perm = crate::syscall::SYSCALL_PERMISSIONS[58];
            (perm.admin, perm.ring_min)
        };
        test_true!(tx);  // syscall 58 (driver_unload) requires admin
    });

    test_case!("se_user_denied_admin_syscall", {
        let user_token = Token::new_user();
        test_true!(!user_token.is_admin_token());

        let result = crate::syscall::check_syscall_permission(58, false);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), crate::syscall::err_to_u64(crate::syscall::SyscallError::Perm));
    });

    test_case!("se_admin_token_isolation", {
        let admin = Token::new_admin();
        let user = Token::new_user();

        test_true!(admin.is_admin_token());
        test_true!(!user.is_admin_token());

        test_ne!(admin.sid, user.sid);

        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(user.sid, ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        // User can read
        test_true!(se_access_check(&user, Some(&sd), ACCESS_READ));
        // User cannot write
        test_true!(!se_access_check(&user, Some(&sd), ACCESS_WRITE));
        // Admin bypasses all
        test_true!(se_access_check(&admin, Some(&sd), ACCESS_ALL));
    });

    // ── NT6.5: NT-correct deny-first semantics ──

    test_case!("se_deny_first_allow_after_deny", {
        // Deny ACE after Allow ACE in the list — deny must still win
        let user = sid_builtin_user();
        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(user, ACCESS_READ));
        acl.add_ace(Ace::deny(user, ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        let token = Token::new_user();
        // Deny occurs later in the list but must be evaluated first
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_READ));
    });

    test_case!("se_deny_first_mixed_aces", {
        let user = sid_builtin_user();
        let admin_s = sid_builtin_admin();
        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(admin_s, ACCESS_ALL));
        acl.add_ace(Ace::allow(user, ACCESS_WRITE));
        acl.add_ace(Ace::deny(user, ACCESS_READ));
        acl.add_ace(Ace::allow(user, ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        let token = Token::new_user();
        // Deny ACE for READ is checked first (before Allow READ at end)
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_READ));
        // No deny for WRITE → Allow wins
        test_true!(se_access_check(&token, Some(&sd), ACCESS_WRITE));
    });

    test_case!("se_insert_ace_canonical", {
        let user = sid_builtin_user();
        let mut acl = Acl::new();
        acl.insert_ace_canonical(Ace::allow(user, ACCESS_READ));
        acl.insert_ace_canonical(Ace::deny(user, ACCESS_WRITE));
        acl.insert_ace_canonical(Ace::allow(user, ACCESS_EXECUTE));
        // Order should be: deny(WRITE), allow(READ), allow(EXECUTE)
        test_eq!(acl.aces.len(), 3);
        test_eq!(acl.aces[0].ace_type, ACE_TYPE_ACCESS_DENIED);
        test_eq!(acl.aces[1].ace_type, ACE_TYPE_ACCESS_ALLOWED);
        test_eq!(acl.aces[2].ace_type, ACE_TYPE_ACCESS_ALLOWED);
        test_eq!(acl.aces[0].access_mask, ACCESS_WRITE);
        test_eq!(acl.aces[1].access_mask, ACCESS_READ);
        test_eq!(acl.aces[2].access_mask, ACCESS_EXECUTE);
    });

    // ── USR-P1d: empty/NULL DACL, group SIDs, SACL audit ──

    test_case!("se_empty_dacl_denies", {
        // An empty DACL (present, zero ACEs) denies all access (NT semantics).
        let sd = SecurityDescriptor::new().with_dacl(Acl::new());
        let token = Token::new_user();
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_READ));
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_ALL));
    });

    test_case!("se_null_dacl_allows", {
        // A NULL DACL (`dacl == None`) grants full access.
        let sd = SecurityDescriptor::new();
        let token = Token::new_user();
        test_true!(se_access_check(&token, Some(&sd), ACCESS_READ));
        test_true!(se_access_check(&token, Some(&sd), ACCESS_ALL));
    });

    test_case!("se_absent_sd_allows", {
        // No descriptor at all is treated as unprotected.
        let token = Token::new_user();
        test_true!(se_access_check(&token, None, ACCESS_READ));
        test_true!(se_access_check(&token, None, ACCESS_ALL));
    });

    test_case!("se_group_sid_allow", {
        // Grant via a group SID the token belongs to.
        let group = Sid::from_parts(1, &[0, 0, 0, 0, 0, 5], &[32, 544]);
        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(group, ACCESS_READ | ACCESS_WRITE));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        let mut token = Token::new_user();
        // Not a member yet → denied.
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_READ));
        // Member → granted for the allowed bits only.
        token.add_group(group);
        test_true!(se_access_check(&token, Some(&sd), ACCESS_READ));
        test_true!(se_access_check(&token, Some(&sd), ACCESS_WRITE));
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_EXECUTE));
    });

    test_case!("se_group_sid_deny_wins", {
        // A group Deny ACE beats a primary-SID Allow ACE (deny-first).
        let group = Sid::from_parts(1, &[0, 0, 0, 0, 0, 5], &[32, 544]);
        let user = sid_builtin_user();
        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(user, ACCESS_ALL));
        acl.add_ace(Ace::deny(group, ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl);

        let mut token = Token::new_user();
        token.add_group(group);
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_READ));
        // WRITE is not covered by the group deny → allowed.
        test_true!(se_access_check(&token, Some(&sd), ACCESS_WRITE));
    });

    test_case!("se_audit_absent_sacl_is_noop", {
        // Auditing with no SACL must be a safe no-op and never change decisions.
        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(sid_builtin_user(), ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl);
        let token = Token::new_user();

        se_audit(&token, Some(&sd), ACCESS_READ, true); // off by default
        set_auditing(true);
        se_audit(&token, Some(&sd), ACCESS_READ, true); // no SACL → still safe
        set_auditing(false);

        test_true!(se_access_check(&token, Some(&sd), ACCESS_READ));
    });

    test_case!("se_acl_sacl_present", {
        // A descriptor can carry a SACL without affecting the DACL decision.
        let mut sacl = Acl::new();
        sacl.add_ace(Ace { ace_type: ACE_TYPE_SYSTEM_AUDIT, flags: 0,
                           access_mask: ACCESS_READ, sid: sid_builtin_user() });
        let mut acl = Acl::new();
        acl.add_ace(Ace::allow(sid_builtin_user(), ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl).with_sacl(sacl);

        test_true!(sd.sacl.is_some());
        let token = Token::new_user();
        test_true!(se_access_check(&token, Some(&sd), ACCESS_READ));
        test_true!(!se_access_check(&token, Some(&sd), ACCESS_WRITE));
    });
}
