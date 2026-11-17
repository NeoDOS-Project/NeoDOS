use core::fmt;

pub const SID_REVISION: u8 = 1;
pub const MAX_SUB_AUTHORITIES: usize = 8;

/// Well-known RIDs within the built-in account domain (S-1-5-21-0-0-0).
pub const RID_ADMINISTRATOR: u32 = 500;
pub const RID_GUEST: u32 = 501;
pub const RID_USER: u32 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sid {
    pub revision: u8,
    pub sub_authority_count: u8,
    pub identifier_authority: [u8; 6],
    pub sub_authorities: [u32; MAX_SUB_AUTHORITIES],
}

impl Sid {
    pub const fn new() -> Self {
        Sid {
            revision: SID_REVISION,
            sub_authority_count: 0,
            identifier_authority: [0; 6],
            sub_authorities: [0; MAX_SUB_AUTHORITIES],
        }
    }

    pub fn from_parts(revision: u8, authority: &[u8; 6], sub_authorities: &[u32]) -> Self {
        let mut sid = Sid::new();
        sid.revision = revision;
        sid.identifier_authority.copy_from_slice(authority);
        let count = sub_authorities.len().min(MAX_SUB_AUTHORITIES);
        sid.sub_authority_count = count as u8;
        for (i, &sa) in sub_authorities.iter().enumerate().take(count) {
            sid.sub_authorities[i] = sa;
        }
        sid
    }

    pub fn format_string(&self) -> alloc::string::String {
        let authority = u64::from_be_bytes([
            0, 0,
            self.identifier_authority[0],
            self.identifier_authority[1],
            self.identifier_authority[2],
            self.identifier_authority[3],
            self.identifier_authority[4],
            self.identifier_authority[5],
        ]);
        let mut s = alloc::format!("S-{}-{}", self.revision, authority);
        for i in 0..self.sub_authority_count as usize {
            s = alloc::format!("{}-{}", s, self.sub_authorities[i]);
        }
        s
    }
}

impl fmt::Display for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_string())
    }
}

pub fn sid_builtin_admin() -> Sid {
    Sid::from_parts(1, &[0, 0, 0, 0, 0, 5], &[18])
}

/// NT AUTHORITY\SYSTEM (`LOCAL_SYSTEM`, `S-1-5-18`).
pub fn sid_builtin_system() -> Sid {
    sid_builtin_admin()
}

/// Built-in account domain base: `S-1-5-21-0-0-0`.
pub fn sid_builtin_domain() -> Sid {
    Sid::from_parts(1, &[0, 0, 0, 0, 0, 5], &[21, 0, 0, 0])
}

/// Built-in `Administrator` account: `S-1-5-21-0-0-0-500`.
pub fn sid_builtin_administrator() -> Sid {
    Sid::from_parts(1, &[0, 0, 0, 0, 0, 5], &[21, 0, 0, 0, RID_ADMINISTRATOR])
}

/// Built-in `Guest` account: `S-1-5-21-0-0-0-501`.
pub fn sid_builtin_guest() -> Sid {
    Sid::from_parts(1, &[0, 0, 0, 0, 0, 5], &[21, 0, 0, 0, RID_GUEST])
}

pub fn sid_builtin_user() -> Sid {
    Sid::from_parts(1, &[0, 0, 0, 0, 0, 5], &[21, 0, 0, 0, RID_USER])
}

lazy_static::lazy_static! {
    /// `SeWellKnownSids` — canonical name → SID table (NT well-known SIDs).
    pub static ref WELL_KNOWN_SIDS: alloc::vec::Vec<(&'static str, Sid)> = alloc::vec![
        ("SYSTEM", sid_builtin_system()),
        ("Administrator", sid_builtin_administrator()),
        ("Guest", sid_builtin_guest()),
        ("Users", sid_builtin_user()),
    ];
}

/// Look up a well-known SID by (case-insensitive) name.
pub fn well_known_sid(name: &str) -> Option<Sid> {
    WELL_KNOWN_SIDS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, s)| *s)
}
