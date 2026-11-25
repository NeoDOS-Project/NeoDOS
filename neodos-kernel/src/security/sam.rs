use crate::security::sid::{Sid, MAX_SUB_AUTHORITIES};
use alloc::string::String;
use alloc::vec::Vec;
use libcrypto::sha256::Sha256;

// ── Binary format constants ─────────────────────────────────────────

const SAM_MAGIC: [u8; 4] = [b'S', b'A', b'M', 0];
const SAM_VERSION: u32 = 2;
const MAX_SAM_ENTRIES: u32 = 64;

pub const SAM_FLAG_ADMIN: u32 = 1;
pub const SAM_FLAG_DISABLED: u32 = 2;
pub const SAM_FLAG_LOCKED: u32 = 4;
/// The entry carries a password hash (otherwise only the empty password is
/// accepted).
pub const SAM_FLAG_PASSWORD_SET: u32 = 8;

/// Salt length for password hashes.
pub const SAM_SALT_LEN: usize = 16;

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
    BadPassword,
    AccountDisabled,
}

// ── Entry ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SamEntry {
    pub username: String,
    pub sid: Sid,
    pub flags: u32,
    pub full_name: String,
    pub comment: String,
    /// Per-entry salt; random when the CSPRNG is available, otherwise derived
    /// from the username.
    pub password_salt: [u8; SAM_SALT_LEN],
    /// `SHA-256(salt || password)` when `SAM_FLAG_PASSWORD_SET` is set.
    pub password_hash: [u8; 32],
}

impl SamEntry {
    pub fn new(username: &str, sid: Sid, is_admin: bool) -> Self {
        SamEntry {
            username: String::from(username),
            sid,
            flags: if is_admin { SAM_FLAG_ADMIN } else { 0 },
            full_name: String::new(),
            comment: String::new(),
            password_salt: [0u8; SAM_SALT_LEN],
            password_hash: [0u8; 32],
        }
    }

    pub fn has_password(&self) -> bool {
        self.flags & SAM_FLAG_PASSWORD_SET != 0
    }

    /// Set (or replace) the account password. The plaintext is never stored.
    pub fn set_password(&mut self, password: &[u8]) {
        let mut salt = [0u8; SAM_SALT_LEN];
        if crate::random::fill(&mut salt).is_err() {
            // Before the CSPRNG is seeded the salt is deterministic per user;
            // it still prevents a single rainbow table from covering all
            // accounts.
            let digest = Sha256::digest(self.username.as_bytes());
            salt.copy_from_slice(&digest[..SAM_SALT_LEN]);
        }
        self.password_salt = salt;
        self.password_hash = hash_password(&salt, password);
        self.flags |= SAM_FLAG_PASSWORD_SET;
    }

    /// Constant-time password check. Accounts without a password accept only
    /// the empty string.
    pub fn verify_password(&self, password: &[u8]) -> bool {
        if !self.has_password() {
            return password.is_empty();
        }
        let candidate = hash_password(&self.password_salt, password);
        let mut diff = 0u8;
        for i in 0..32 {
            diff |= self.password_hash[i] ^ candidate[i];
        }
        diff == 0
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

    /// Create the built-in account database: `Administrator` (enabled, admin),
    /// `Guest` (disabled), and `SYSTEM` (admin). Mirrors the NT built-in SAM
    /// accounts; entries that do not fit are ignored.
    pub fn with_builtins() -> Self {
        use crate::security::sid::{
            sid_builtin_administrator, sid_builtin_guest, sid_builtin_system, sid_builtin_user,
        };

        let mut db = SamDatabase::new();

        let mut admin = SamEntry::new("Administrator", sid_builtin_administrator(), true);
        admin.full_name = String::from("Administrator");
        admin.comment = String::from("Built-in administrative account");
        let _ = db.add_user(admin);

        let mut guest = SamEntry::new("Guest", sid_builtin_guest(), false);
        guest.flags |= SAM_FLAG_DISABLED;
        guest.full_name = String::from("Guest");
        guest.comment = String::from("Built-in guest account (disabled)");
        let _ = db.add_user(guest);

        let mut system = SamEntry::new("SYSTEM", sid_builtin_system(), true);
        system.full_name = String::from("Local System");
        system.comment = String::from("Operating system account");
        let _ = db.add_user(system);

        // Default interactive account until the logon/session layer (#37/#40)
        // issues per-user tokens. It is deliberately not an administrator.
        let mut user = SamEntry::new("NeoDOS", sid_builtin_user(), false);
        user.full_name = String::from("NeoDOS User");
        user.comment = String::from("Default unprivileged interactive account");
        let _ = db.add_user(user);

        // Local operator account used to exercise the logon/runas flow (#37/#40).
        let mut local = SamEntry::new(
            "amartinper",
            crate::security::sid::sid_builtin_account(1001),
            false,
        );
        local.full_name = String::from("Alejandro Martin");
        local.comment = String::from("Local unprivileged account");
        let _ = db.add_user(local);

        db
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

    /// Install the documented development credentials on first use.
    ///
    /// SAM is RAM-only, so the built-in demo accounts start without a password
    /// on every boot. Seeding is lazy (first logon) instead of at boot: it runs
    /// in user context with the CSPRNG already initialized, and it avoids
    /// extending the early-boot crypto window.
    pub fn ensure_default_credentials(&mut self) {
        for (name, password) in [("amartinper", b"neodos".as_slice()), ("Administrator", b"neodos".as_slice())] {
            if let Some(entry) = self.find_by_username_mut(name) {
                if !entry.has_password() {
                    entry.set_password(password);
                }
            }
        }
    }

    /// Verify a username/password pair. Disabled and locked accounts never
    /// authenticate.
    pub fn authenticate(&self, name: &str, password: &[u8]) -> Result<&SamEntry, SamError> {
        let entry = self.find_by_username(name).ok_or(SamError::EntryNotFound)?;
        if entry.is_disabled() {
            return Err(SamError::AccountDisabled);
        }
        if entry.is_locked() {
            return Err(SamError::AccountDisabled);
        }
        if !entry.verify_password(password) {
            return Err(SamError::BadPassword);
        }
        Ok(entry)
    }

    pub fn is_admin(&self, name: &str) -> bool {
        self.find_by_username(name).map_or(false, |e| e.is_admin())
    }

    pub fn entry_count(&self) -> u32 {
        self.entries.len() as u32
    }
}

fn hash_password(salt: &[u8; SAM_SALT_LEN], password: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(password);
    hasher.finalize()
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

        if offset + SAM_SALT_LEN + 32 > data.len() {
            return Err(SamError::Truncated);
        }
        let mut password_salt = [0u8; SAM_SALT_LEN];
        password_salt.copy_from_slice(&data[offset..offset + SAM_SALT_LEN]);
        offset += SAM_SALT_LEN;
        let mut password_hash = [0u8; 32];
        password_hash.copy_from_slice(&data[offset..offset + 32]);
        offset += 32;

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
            password_salt,
            password_hash,
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

        // password salt + hash (v2)
        buf.extend_from_slice(&entry.password_salt);
        buf.extend_from_slice(&entry.password_hash);

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
    use crate::security::sid::{sid_builtin_administrator, sid_builtin_user};

    test_case!("sam_create_database", {
        let db = SamDatabase::new();
        test_eq!(db.entry_count(), 0);
    });

    test_case!("sam_add_user", {
        let mut db = SamDatabase::new();
        let admin_sid = sid_builtin_administrator();
        let entry = SamEntry::new("Admin", admin_sid, true);
        db.add_user(entry).unwrap();
        test_eq!(db.entry_count(), 1);
        test_true!(db.is_admin("Admin"));
    });

    test_case!("sam_find_by_username", {
        let mut db = SamDatabase::new();
        db.add_user(SamEntry::new("Admin", sid_builtin_administrator(), true)).unwrap();
        db.add_user(SamEntry::new("User1", sid_builtin_user(), false)).unwrap();

        let found = db.find_by_username("admin").unwrap();
        test_true!(found.is_admin());
        test_eq!(found.username, "Admin");

        let found2 = db.find_by_username("user1").unwrap();
        test_false!(found2.is_admin());
    });

    test_case!("sam_find_by_sid", {
        let mut db = SamDatabase::new();
        let admin_sid = sid_builtin_administrator();
        db.add_user(SamEntry::new("Admin", admin_sid, true)).unwrap();

        let found = db.find_by_sid(&admin_sid).unwrap();
        test_eq!(found.username, "Admin");
    });

    test_case!("sam_remove_user", {
        let mut db = SamDatabase::new();
        db.add_user(SamEntry::new("Admin", sid_builtin_administrator(), true)).unwrap();
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
        db.add_user(SamEntry::new("Admin", sid_builtin_administrator(), true)).unwrap();
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
        test_eq!(admin.sid, sid_builtin_administrator());

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
        let mut bad = [0u8; 16];
        bad[0..4].copy_from_slice(&SAM_MAGIC);
        bad[4..8].copy_from_slice(&SAM_VERSION.to_le_bytes());
        bad[8..12].copy_from_slice(&1u32.to_le_bytes());
        let result = parse_sam(&bad);
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
        db.add_user(SamEntry::new("Administrator", sid_builtin_administrator(), true)).unwrap();
        test_true!(db.find_by_username("ADMINISTRATOR").is_some());
        test_true!(db.find_by_username("administrator").is_some());
        test_true!(db.find_by_username("Administrator").is_some());
    });

    test_case!("sam_builtin_accounts", {
        use crate::security::sid::{
            sid_builtin_administrator, sid_builtin_guest, sid_builtin_system,
        };
        let db = SamDatabase::with_builtins();
        test_eq!(db.entry_count(), 5);

        let admin = db.find_by_username("administrator").unwrap();
        test_true!(admin.is_admin());
        test_true!(!admin.is_disabled());
        test_eq!(admin.sid, sid_builtin_administrator());

        let guest = db.find_by_username("GUEST").unwrap();
        test_true!(guest.is_disabled());
        test_true!(!guest.is_admin());
        test_eq!(guest.sid, sid_builtin_guest());

        let system = db.find_by_username("system").unwrap();
        test_true!(system.is_admin());
        test_eq!(system.sid, sid_builtin_system());

        let user = db.find_by_username("neodos").unwrap();
        test_true!(!user.is_admin());
        test_true!(!user.is_disabled());

        let local = db.find_by_username("AMARTINPER").unwrap();
        test_true!(!local.is_admin());
        test_true!(!local.is_disabled());
        test_eq!(local.sid, crate::security::sid::sid_builtin_account(1001));
    });
}
