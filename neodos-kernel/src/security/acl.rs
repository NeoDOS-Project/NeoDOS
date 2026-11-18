use crate::security::sid::Sid;

pub const ACE_TYPE_ACCESS_ALLOWED: u8 = 0;
pub const ACE_TYPE_ACCESS_DENIED: u8 = 1;
pub const ACE_TYPE_SYSTEM_AUDIT: u8 = 2;

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

    /// Insert ACE in NT-canonical order: all Deny ACEs before all Allow ACEs.
    /// Maintains insertion order within each group.
    pub fn insert_ace_canonical(&mut self, ace: Ace) {
        if ace.ace_type == ACE_TYPE_ACCESS_DENIED {
            // Insert at the first Allow position (or end if no Allow exists)
            let pos = self.aces.iter().position(|a| a.ace_type == ACE_TYPE_ACCESS_ALLOWED)
                .unwrap_or(self.aces.len());
            self.aces.insert(pos, ace);
        } else {
            // Allow: append at end
            self.aces.push(ace);
        }
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
    /// Discretionary ACL. `None` is a NULL DACL (full access); an empty ACL
    /// denies all access (NT semantics).
    pub dacl: Option<Acl>,
    /// System ACL — audit ACEs only. `None` means no auditing configured.
    pub sacl: Option<Acl>,
}

impl SecurityDescriptor {
    pub fn new() -> Self {
        SecurityDescriptor {
            revision: 1,
            owner: None,
            group: None,
            dacl: None,
            sacl: None,
        }
    }

    pub fn with_dacl(mut self, dacl: Acl) -> Self {
        self.dacl = Some(dacl);
        self
    }

    pub fn with_sacl(mut self, sacl: Acl) -> Self {
        self.sacl = Some(sacl);
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

    pub fn set_sacl(&mut self, sacl: Acl) {
        self.sacl = Some(sacl);
    }
}

// ── Binary codec for on-disk security descriptors ──────────────────

/// Version of the on-disk descriptor encoding.
pub const SD_ENCODING_VERSION: u8 = 1;
/// Upper bound accepted from disk; a file DACL larger than this is rejected.
pub const MAX_SECURITY_DESCRIPTOR_BYTES: usize = 1024;
const SD_MAGIC: [u8; 4] = *b"NDSD";
const MAX_ACES: usize = 32;

/// Serialize an NT security descriptor (owner, group, DACL, SACL).
pub fn serialize_security_descriptor(sd: &SecurityDescriptor) -> Option<alloc::vec::Vec<u8>> {
    let mut out = alloc::vec::Vec::with_capacity(128);
    out.extend_from_slice(&SD_MAGIC);
    out.push(SD_ENCODING_VERSION);
    out.push(sd.revision);

    write_optional_sid(&mut out, sd.owner.as_ref())?;
    write_optional_sid(&mut out, sd.group.as_ref())?;
    write_acl(&mut out, sd.dacl.as_ref())?;
    write_acl(&mut out, sd.sacl.as_ref())?;

    if out.len() > MAX_SECURITY_DESCRIPTOR_BYTES {
        return None;
    }
    Some(out)
}

fn write_optional_sid(out: &mut alloc::vec::Vec<u8>, sid: Option<&Sid>) -> Option<()> {
    match sid {
        None => out.push(0),
        Some(sid) => {
            let mut raw = [0u8; 40];
            let len = sid.write_binary(&mut raw);
            out.push(1);
            out.push(len as u8);
            out.extend_from_slice(&raw[..len]);
        }
    }
    Some(())
}

fn write_acl(out: &mut alloc::vec::Vec<u8>, acl: Option<&Acl>) -> Option<()> {
    match acl {
        None => out.push(0),
        Some(acl) => {
            if acl.aces.len() > MAX_ACES {
                return None;
            }
            out.push(1);
            out.push(acl.revision);
            out.extend_from_slice(&(acl.aces.len() as u16).to_le_bytes());
            for ace in &acl.aces {
                out.push(ace.ace_type);
                out.push(ace.flags);
                out.extend_from_slice(&ace.access_mask.to_le_bytes());
                let mut raw = [0u8; 40];
                let len = ace.sid.write_binary(&mut raw);
                out.push(len as u8);
                out.extend_from_slice(&raw[..len]);
            }
        }
    }
    Some(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }
    fn u8(&mut self) -> Option<u8> {
        let v = *self.bytes.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.bytes.len() {
            return None;
        }
        let s = &self.bytes[self.pos..end];
        self.pos = end;
        Some(s)
    }
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }
}

fn read_optional_sid(r: &mut Reader<'_>) -> Option<Option<Sid>> {
    match r.u8()? {
        0 => Some(None),
        1 => {
            let len = r.u8()? as usize;
            let raw = r.take(len)?;
            Some(Some(Sid::from_binary(raw)?))
        }
        _ => None,
    }
}

fn read_acl(r: &mut Reader<'_>) -> Option<Option<Acl>> {
    match r.u8()? {
        0 => Some(None),
        1 => {
            let revision = r.u8()?;
            let count = r.u16()? as usize;
            if count > MAX_ACES {
                return None;
            }
            let mut acl = Acl::new();
            acl.revision = revision;
            for _ in 0..count {
                let ace_type = r.u8()?;
                let flags = r.u8()?;
                let access_mask = r.u32()?;
                let len = r.u8()? as usize;
                let raw = r.take(len)?;
                let sid = Sid::from_binary(raw)?;
                acl.aces.push(Ace { ace_type, flags, access_mask, sid });
            }
            Some(Some(acl))
        }
        _ => None,
    }
}

/// Parse an on-disk NT security descriptor. Strict: unknown version, bad
/// magic, truncation or trailing bytes are rejected.
pub fn deserialize_security_descriptor(bytes: &[u8]) -> Option<SecurityDescriptor> {
    if bytes.len() < 6 || bytes.len() > MAX_SECURITY_DESCRIPTOR_BYTES {
        return None;
    }
    let mut r = Reader::new(bytes);
    if r.take(4)? != SD_MAGIC || r.u8()? != SD_ENCODING_VERSION {
        return None;
    }
    let revision = r.u8()?;
    let owner = read_optional_sid(&mut r)?;
    let group = read_optional_sid(&mut r)?;
    let dacl = read_acl(&mut r)?;
    let sacl = read_acl(&mut r)?;
    if r.remaining() != 0 {
        return None;
    }
    Some(SecurityDescriptor { revision, owner, group, dacl, sacl })
}
