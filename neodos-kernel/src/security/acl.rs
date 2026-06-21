use crate::security::sid::Sid;

pub const ACE_TYPE_ACCESS_ALLOWED: u8 = 0;
pub const ACE_TYPE_ACCESS_DENIED: u8 = 1;

pub const ACCESS_READ: u32 = 1;
pub const ACCESS_WRITE: u32 = 2;
pub const ACCESS_EXECUTE: u32 = 4;
pub const ACCESS_DELETE: u32 = 8;
pub const ACCESS_ALL: u32 = 0xFFFF;

#[derive(Debug, Clone, Copy)]
pub struct Ace {
    pub ace_type: u8,
    pub flags: u8,
    pub access_mask: u32,
    pub sid: Sid,
}

impl Ace {
    pub fn allow(sid: Sid, access_mask: u32) -> Self {
        Ace { ace_type: ACE_TYPE_ACCESS_ALLOWED, flags: 0, access_mask, sid }
    }

    pub fn deny(sid: Sid, access_mask: u32) -> Self {
        Ace { ace_type: ACE_TYPE_ACCESS_DENIED, flags: 0, access_mask, sid }
    }
}

#[derive(Debug, Clone)]
pub struct Acl {
    pub revision: u8,
    pub aces: alloc::vec::Vec<Ace>,
}

impl Acl {
    pub fn new() -> Self {
        Acl { revision: 2, aces: alloc::vec::Vec::new() }
    }

    pub fn add_ace(&mut self, ace: Ace) {
        self.aces.push(ace);
    }

    pub fn is_empty(&self) -> bool {
        self.aces.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct SecurityDescriptor {
    pub revision: u8,
    pub owner: Option<Sid>,
    pub group: Option<Sid>,
    pub dacl: Option<Acl>,
}

impl SecurityDescriptor {
    pub fn new() -> Self {
        SecurityDescriptor {
            revision: 1,
            owner: None,
            group: None,
            dacl: None,
        }
    }

    pub fn with_dacl(mut self, dacl: Acl) -> Self {
        self.dacl = Some(dacl);
        self
    }

    pub fn set_owner(&mut self, sid: Sid) {
        self.owner = Some(sid);
    }

    pub fn set_group(&mut self, sid: Sid) {
        self.group = Some(sid);
    }

    pub fn set_dacl(&mut self, dacl: Acl) {
        self.dacl = Some(dacl);
    }
}
