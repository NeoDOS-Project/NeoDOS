use crate::security::acl::{Acl, SecurityDescriptor, ACE_TYPE_ACCESS_ALLOWED, ACE_TYPE_ACCESS_DENIED};
use crate::security::token::Token;
use crate::security::sid::Sid;

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

    for ace in &dacl.aces {
        if ace.sid == token.sid {
            if (ace.access_mask & desired_access) == desired_access {
                match ace.ace_type {
                    ACE_TYPE_ACCESS_DENIED => return false,
                    ACE_TYPE_ACCESS_ALLOWED => return true,
                    _ => {}
                }
            }
        }
    }

    false
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
    for ace in &dacl.aces {
        if &ace.sid == token_sid {
            if (ace.access_mask & desired_access) == desired_access {
                match ace.ace_type {
                    ACE_TYPE_ACCESS_DENIED => return false,
                    ACE_TYPE_ACCESS_ALLOWED => return true,
                    _ => {}
                }
            }
        }
    }
    false
}
