use crate::security::sid::{Sid, MAX_SUB_AUTHORITIES};
use alloc::string::String;
use alloc::vec::Vec;

// ── Binary format constants ─────────────────────────────────────────

const SAM_MAGIC: [u8; 4] = [b'S', b'A', b'M', 0];
const SAM_VERSION: u32 = 1;
const MAX_SAM_ENTRIES: u32 = 64;

pub const SAM_FLAG_ADMIN: u32 = 1;
pub const SAM_FLAG_DISABLED: u32 = 2;
pub const SAM_FLAG_LOCKED: u32 = 4;

// ── Error type ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamError {
    BadMagic,
    BadVersion,
    Truncated,
    TooManyEntries,
    InvalidSid,
    UsernameTooLong,
    EntryNotFound,
}

// ── Entry ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SamEntry {
    pub username: String,
    pub sid: Sid,
    pub flags: u32,
    pub full_name: String,
    pub comment: String,
}

impl SamEntry {
    pub fn new(username: &str, sid: Sid, is_admin: bool) -> Self {
        SamEntry {
            username: String::from(username),
            sid,
            flags: if is_admin { SAM_FLAG_ADMIN } else { 0 },
            full_name: String::new(),
            comment: String::new(),
        }
    }

    pub fn is_admin(&self) -> bool {
        self.flags & SAM_FLAG_ADMIN != 0
    }

    pub fn is_disabled(&self) -> bool {
        self.flags & SAM_FLAG_DISABLED != 0
    }

    pub fn is_locked(&self) -> bool {
        self.flags & SAM_FLAG_LOCKED != 0
    }
}

// ── Database ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SamDatabase {
    pub entries: Vec<SamEntry>,
}

impl SamDatabase {
    pub fn new() -> Self {
        SamDatabase { entries: Vec::new() }
    }

    pub fn add_user(&mut self, entry: SamEntry) -> Result<(), SamError> {
        if self.entries.len() >= MAX_SAM_ENTRIES as usize {
            return Err(SamError::TooManyEntries);
        }
        self.entries.push(entry);
        Ok(())
    }

    pub fn find_by_username(&self, name: &str) -> Option<&SamEntry> {
        self.entries.iter().find(|e| e.username.eq_ignore_ascii_case(name))
    }

    pub fn find_by_username_mut(&mut self, name: &str) -> Option<&mut SamEntry> {
        self.entries.iter_mut().find(|e| e.username.eq_ignore_ascii_case(name))
    }

    pub fn find_by_sid(&self, sid: &Sid) -> Option<&SamEntry> {
        self.entries.iter().find(|e| e.sid == *sid)
    }

    pub fn remove_user(&mut self, name: &str) -> bool {
        let idx = self.entries.iter().position(|e| e.username.eq_ignore_ascii_case(name));
        if let Some(i) = idx {
            self.entries.remove(i);
            true
        } else {
            false
        }
    }

    pub fn is_admin(&self, name: &str) -> bool {
        self.find_by_username(name).map_or(false, |e| e.is_admin())
    }

    pub fn entry_count(&self) -> u32 {
        self.entries.len() as u32
    }
}

// ── Binary serialisation helpers ────────────────────────────────────

fn pad_to_4(n: usize) -> usize {
    (n + 3) & !3
}

// ── Parse (load from disk) ──────────────────────────────────────────

pub fn parse_sam(data: &[u8]) -> Result<SamDatabase, SamError> {
    if data.len() < 16 {
        return Err(SamError::Truncated);
    }
    if data[0..4] != SAM_MAGIC {
        return Err(SamError::BadMagic);
    }
    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if version != SAM_VERSION {
        return Err(SamError::BadVersion);
    }
    let count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    if count > MAX_SAM_ENTRIES {
        return Err(SamError::TooManyEntries);
    }

    let mut db = SamDatabase::new();
    let mut offset: usize = 16;

    for _ in 0..count {
        if offset + 2 > data.len() {
            return Err(SamError::Truncated);
        }
        let name_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        if offset + name_len > data.len() {
            return Err(SamError::Truncated);
        }
        let username = core::str::from_utf8(&data[offset..offset + name_len])
            .map_err(|_| SamError::InvalidSid)?;
        offset += name_len;
        offset = pad_to_4(offset);

        if offset + 8 > data.len() {
            return Err(SamError::Truncated);
        }
        let sid_revision = data[offset];
        let sid_sub_count = data[offset + 1] as usize;
        let mut sid_authority: [u8; 6] = [0; 6];
        sid_authority.copy_from_slice(&data[offset + 2..offset + 8]);
        offset += 8;

        if sid_sub_count > MAX_SUB_AUTHORITIES || offset + sid_sub_count * 4 > data.len() {
            return Err(SamError::Truncated);
        }
        let mut sid_subs: [u32; MAX_SUB_AUTHORITIES] = [0; MAX_SUB_AUTHORITIES];
        for i in 0..sid_sub_count {
            let b = &data[offset + i * 4..offset + i * 4 + 4];
            sid_subs[i] = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        }
        offset += sid_sub_count * 4;
        offset = pad_to_4(offset);

        if offset + 4 > data.len() {
            return Err(SamError::Truncated);
        }
        let flags = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
        offset += 4;

        if offset + 2 > data.len() {
            return Err(SamError::Truncated);
        }
        let fn_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        if offset + fn_len > data.len() {
            return Err(SamError::Truncated);
        }
        let full_name = core::str::from_utf8(&data[offset..offset + fn_len])
            .map_err(|_| SamError::InvalidSid)?;
        offset += fn_len;
        offset = pad_to_4(offset);

        if offset + 2 > data.len() {
            return Err(SamError::Truncated);
        }
        let cmt_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        if offset + cmt_len > data.len() {
            return Err(SamError::Truncated);
        }
        let comment = core::str::from_utf8(&data[offset..offset + cmt_len])
            .map_err(|_| SamError::InvalidSid)?;
        offset += cmt_len;
        offset = pad_to_4(offset);

        db.entries.push(SamEntry {
            username: String::from(username),
            sid: Sid::from_parts(sid_revision, &sid_authority, &sid_subs[..sid_sub_count]),
            flags,
            full_name: String::from(full_name),
            comment: String::from(comment),
        });
    }

    Ok(db)
}

// ── Serialise (save to disk) ────────────────────────────────────────

pub fn serialize_sam(db: &SamDatabase) -> Result<Vec<u8>, SamError> {
    let mut buf = Vec::new();
    // Header
    buf.extend_from_slice(&SAM_MAGIC);
    buf.extend_from_slice(&SAM_VERSION.to_le_bytes());
    buf.extend_from_slice(&(db.entries.len() as u32).to_le_bytes());
    buf.extend_from_slice(&[0u8; 4]); // reserved

    for entry in &db.entries {
        // username
        let uname = entry.username.as_bytes();
        buf.extend_from_slice(&(uname.len() as u16).to_le_bytes());
        buf.extend_from_slice(uname);
        while buf.len() % 4 != 0 { buf.push(0); }

        // sid
        buf.push(entry.sid.revision);
        buf.push(entry.sid.sub_authority_count);
        buf.extend_from_slice(&entry.sid.identifier_authority);
        for i in 0..entry.sid.sub_authority_count as usize {
            buf.extend_from_slice(&entry.sid.sub_authorities[i].to_le_bytes());
        }
        while buf.len() % 4 != 0 { buf.push(0); }

        // flags
        buf.extend_from_slice(&entry.flags.to_le_bytes());

        // full_name
        let fname = entry.full_name.as_bytes();
        buf.extend_from_slice(&(fname.len() as u16).to_le_bytes());
        buf.extend_from_slice(fname);
        while buf.len() % 4 != 0 { buf.push(0); }

        // comment
        let cmt = entry.comment.as_bytes();
        buf.extend_from_slice(&(cmt.len() as u16).to_le_bytes());
        buf.extend_from_slice(cmt);
        while buf.len() % 4 != 0 { buf.push(0); }
    }

    Ok(buf)
}

// ── Tests ───────────────────────────────────────────────────────────

pub fn register_sam_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;
    use crate::test_false;
    use crate::security::sid::{sid_builtin_admin, sid_builtin_user};

    test_case!("sam_create_database", {
        let db = SamDatabase::new();
        test_eq!(db.entry_count(), 0);
    });

    test_case!("sam_add_user", {
        let mut db = SamDatabase::new();
        let admin_sid = sid_builtin_admin();
        let entry = SamEntry::new("Admin", admin_sid, true);
        db.add_user(entry).unwrap();
        test_eq!(db.entry_count(), 1);
        test_true!(db.is_admin("Admin"));
    });

    test_case!("sam_find_by_username", {
        let mut db = SamDatabase::new();
        db.add_user(SamEntry::new("Admin", sid_builtin_admin(), true)).unwrap();
        db.add_user(SamEntry::new("User1", sid_builtin_user(), false)).unwrap();

        let found = db.find_by_username("admin").unwrap();
        test_true!(found.is_admin());
        test_eq!(found.username, "Admin");

        let found2 = db.find_by_username("user1").unwrap();
        test_false!(found2.is_admin());
    });

    test_case!("sam_find_by_sid", {
        let mut db = SamDatabase::new();
        let admin_sid = sid_builtin_admin();
        db.add_user(SamEntry::new("Admin", admin_sid, true)).unwrap();

        let found = db.find_by_sid(&admin_sid).unwrap();
        test_eq!(found.username, "Admin");
    });

    test_case!("sam_remove_user", {
        let mut db = SamDatabase::new();
        db.add_user(SamEntry::new("Admin", sid_builtin_admin(), true)).unwrap();
        db.add_user(SamEntry::new("Temp", sid_builtin_user(), false)).unwrap();
        test_eq!(db.entry_count(), 2);

        test_true!(db.remove_user("Temp"));
        test_eq!(db.entry_count(), 1);
        test_true!(db.find_by_username("Temp").is_none());
    });

    test_case!("sam_flags_disabled_locked", {
        let mut entry = SamEntry::new("Test", sid_builtin_user(), false);
        entry.flags |= SAM_FLAG_DISABLED | SAM_FLAG_LOCKED;
        test_true!(entry.is_disabled());
        test_true!(entry.is_locked());
        test_false!(entry.is_admin());
    });

    test_case!("sam_parse_roundtrip", {
        let mut db = SamDatabase::new();
        db.add_user(SamEntry::new("Admin", sid_builtin_admin(), true)).unwrap();
        let mut user_entry = SamEntry::new("Alejandro", sid_builtin_user(), false);
        user_entry.full_name = String::from("Alejandro Martin");
        user_entry.comment = String::from("Default user");
        db.add_user(user_entry).unwrap();

        let data = serialize_sam(&db).unwrap();
        test_true!(data.len() > 16);

        let parsed = parse_sam(&data).unwrap();
        test_eq!(parsed.entry_count(), 2);

        let admin = parsed.find_by_username("Admin").unwrap();
        test_true!(admin.is_admin());
        test_eq!(admin.sid, sid_builtin_admin());

        let user = parsed.find_by_username("Alejandro").unwrap();
        test_false!(user.is_admin());
        test_eq!(user.full_name, "Alejandro Martin");
        test_eq!(user.comment, "Default user");
    });

    test_case!("sam_parse_magic_error", {
        let bad = b"XXX\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let result = parse_sam(bad);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), SamError::BadMagic);
    });

    test_case!("sam_parse_truncated", {
        let bad = b"SAM\x00\x01\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00";
        let result = parse_sam(bad);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), SamError::Truncated);
    });

    test_case!("sam_max_entries_enforced", {
        let mut db = SamDatabase::new();
        for i in 0..64 {
            let mut entry = SamEntry::new(&alloc::format!("User{}", i), sid_builtin_user(), false);
            entry.full_name = alloc::format!("User Number {}", i);
            db.add_user(entry).unwrap();
        }
        // 65th should fail
        let extra = SamEntry::new("Extra", sid_builtin_user(), false);
        let result = db.add_user(extra);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), SamError::TooManyEntries);
    });

    test_case!("sam_case_insensitive_lookup", {
        let mut db = SamDatabase::new();
        db.add_user(SamEntry::new("Administrator", sid_builtin_admin(), true)).unwrap();
        test_true!(db.find_by_username("ADMINISTRATOR").is_some());
        test_true!(db.find_by_username("administrator").is_some());
        test_true!(db.find_by_username("Administrator").is_some());
    });
}
