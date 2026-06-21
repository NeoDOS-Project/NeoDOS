use crate::security::sid::{Sid, sid_builtin_admin, sid_builtin_user};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub sid: Sid,
    pub is_admin: bool,
}

impl Token {
    pub fn new(sid: Sid, is_admin: bool) -> Self {
        Token { sid, is_admin }
    }

    pub fn new_admin() -> Self {
        Token {
            sid: sid_builtin_admin(),
            is_admin: true,
        }
    }

    pub fn new_user() -> Self {
        Token {
            sid: sid_builtin_user(),
            is_admin: false,
        }
    }

    pub fn is_admin_token(&self) -> bool {
        self.is_admin
    }
}
