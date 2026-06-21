pub mod sid;
pub mod token;
pub mod acl;
pub mod access;

use lazy_static::lazy_static;

lazy_static! {
    pub static ref DEFAULT_ADMIN_TOKEN: token::Token = token::Token::new_admin();
    pub static ref DEFAULT_USER_TOKEN: token::Token = token::Token::new_user();
}

pub fn init_security() {
    crate::serial_println!("[SEC] Security subsystem initialized");
    crate::serial_println!("[SEC] Admin SID: {}", sid::sid_builtin_admin());
    crate::serial_println!("[SEC] User SID: {}", sid::sid_builtin_user());
}

pub fn register_security_tests() {
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

    test_case!("token_admin_boot_default", {
        let admin_token = Token::new_admin();
        test_true!(admin_token.is_admin_token());
        test_eq!(admin_token.sid, sid_builtin_admin());

        let user_token = Token::new_user();
        test_true!(!user_token.is_admin_token());
        test_eq!(user_token.sid, sid_builtin_user());
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
            let perm = crate::syscall::SYSCALL_PERMISSIONS[50];
            (perm.admin, perm.ring_min)
        };
        test_true!(tx);  // syscall 50 requires admin
    });

    test_case!("se_user_denied_admin_syscall", {
        let user_token = Token::new_user();
        test_true!(!user_token.is_admin_token());

        let result = crate::syscall::check_syscall_permission(50, false);
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
}
