//! Login sessions (#37) and their token binding (#39).
//!
//! A session groups the processes that belong to one authenticated identity.
//! It is an Object Manager object (`ObType::Session`, `\Session\{id}`) whose
//! lifetime owns the token: logging on authenticates against SAM and binds the
//! issued token, logging off drops it and invalidates every token already
//! handed to the session's processes.
//!
//! The manager lives in `security/` (not `syscall/`) so the access check can
//! validate the token's session without depending on the syscall layer.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::security::sam::SamError;
use crate::security::SAM_DB;
use crate::security::sid::Sid;
use crate::security::token::Token;

/// Maximum number of concurrent login sessions. Session ids are slot indexes
/// (`1..=MAX_SESSIONS`) so validity can be tracked in lock-free bitmaps that
/// the access check can consult from any context, including IRQs.
pub const MAX_SESSIONS: usize = 16;

/// Bit `id` is set while `\Session\{id}` exists.
static SESSION_REGISTERED: AtomicU64 = AtomicU64::new(0);
/// Bit `id` is set while the session is `Active` (token usable).
static SESSION_ACTIVE: AtomicU64 = AtomicU64::new(0);

fn bit(id: u32) -> u64 {
    1u64 << id
}

/// Session state machine documented in `docs/design/users-security-design.md`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Active = 0,
    Locked = 1,
    Disconnected = 2,
    LoggedOff = 3,
}

impl SessionState {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Active),
            1 => Some(Self::Locked),
            2 => Some(Self::Disconnected),
            3 => Some(Self::LoggedOff),
            _ => None,
        }
    }
}

/// One login session.
#[derive(Clone, Debug)]
pub struct Session {
    pub id: u32,
    /// Object Manager id of the backing `ObType::Session` object.
    pub ob_id: u64,
    pub state: SessionState,
    pub vt_num: u8,
    pub username: String,
    pub user_sid: Option<Sid>,
    /// Token bound by the last successful logon. Dropped on logoff.
    pub token: Option<Token>,
    /// Monotonic logon sequence (0 until the first logon).
    pub login_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    NotFound,
    AlreadyExists,
    AlreadyActive,
    NotActive,
    Auth(SamError),
}

/// Session table. Kept small and fixed-purpose; one entry per live session.
pub struct SessionManager {
    sessions: Vec<Session>,
    logons: u64,
}

impl SessionManager {
    pub const fn new() -> Self {
        SessionManager {
            sessions: Vec::new(),
            logons: 0,
        }
    }

    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    /// First free slot id (1..=MAX_SESSIONS), if any.
    pub fn next_free_id(&self) -> Option<u32> {
        let used = SESSION_REGISTERED.load(Ordering::Relaxed);
        (1..=MAX_SESSIONS as u32)
            .find(|id| used & bit(*id) == 0 && self.get(*id).is_none())
    }

    pub fn get(&self, id: u32) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    pub fn find_by_ob(&self, ob_id: u64) -> Option<&Session> {
        self.sessions.iter().find(|s| s.ob_id == ob_id)
    }

    pub fn active_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.state == SessionState::Active)
            .count()
    }

    /// Register a freshly created session object. `requested_id == None` picks
    /// the next free id.
    pub fn register(
        &mut self,
        requested_id: Option<u32>,
        ob_id: u64,
        vt_num: u8,
    ) -> Result<u32, SessionError> {
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(SessionError::AlreadyExists);
        }
        let id = match requested_id {
            Some(id) if id == 0 || id as usize > MAX_SESSIONS => {
                return Err(SessionError::AlreadyExists)
            }
            Some(id) => {
                if self.get(id).is_some() || SESSION_REGISTERED.load(Ordering::Relaxed) & bit(id) != 0 {
                    return Err(SessionError::AlreadyExists);
                }
                id
            }
            None => {
                let used = SESSION_REGISTERED.load(Ordering::Relaxed);
                (1..=MAX_SESSIONS as u32)
                    .find(|id| used & bit(*id) == 0 && self.get(*id).is_none())
                    .ok_or(SessionError::AlreadyExists)?
            }
        };
        SESSION_REGISTERED.fetch_or(bit(id), Ordering::Release);
        self.sessions.push(Session {
            id,
            ob_id,
            state: SessionState::LoggedOff,
            vt_num,
            username: String::new(),
            user_sid: None,
            token: None,
            login_tick: 0,
        });
        Ok(id)
    }

    /// Bind an authenticated entry to the session.
    pub fn bind(&mut self, id: u32, entry: &crate::security::sam::SamEntry) -> Result<Token, SessionError> {
        let logons = self.logons + 1;
        let token = Token::from_sam(entry, id);
        let session = self.get_mut(id).ok_or(SessionError::NotFound)?;
        if session.state == SessionState::Active {
            return Err(SessionError::AlreadyActive);
        }
        session.state = SessionState::Active;
        session.username = entry.username.clone();
        session.user_sid = Some(entry.sid);
        session.login_tick = logons;
        session.token = Some(token.clone());
        self.logons = logons;
        SESSION_ACTIVE.fetch_or(bit(id), Ordering::Release);
        Ok(token)
    }

    pub fn set_state(&mut self, id: u32, state: SessionState) -> Result<(), SessionError> {
        let session = self.get_mut(id).ok_or(SessionError::NotFound)?;
        if session.state == SessionState::LoggedOff && state != SessionState::LoggedOff {
            return Err(SessionError::NotActive);
        }
        session.state = state;
        if state == SessionState::Active {
            SESSION_ACTIVE.fetch_or(bit(id), Ordering::Release);
        } else {
            SESSION_ACTIVE.fetch_and(!bit(id), Ordering::Release);
        }
        Ok(())
    }

    /// Drop the bound token and invalidate every token already handed out for
    /// this session. The entry stays registered until the object is destroyed.
    pub fn logoff(&mut self, id: u32) -> Result<(), SessionError> {
        let session = self.get_mut(id).ok_or(SessionError::NotFound)?;
        session.state = SessionState::LoggedOff;
        session.token = None;
        session.user_sid = None;
        session.login_tick = 0;
        SESSION_ACTIVE.fetch_and(!bit(id), Ordering::Release);
        Ok(())
    }

    /// Session id backing an Object Manager object.
    pub fn id_for_ob(&self, ob_id: u64) -> Option<u32> {
        self.find_by_ob(ob_id).map(|s| s.id)
    }

    pub fn remove_by_ob(&mut self, ob_id: u64) -> bool {
        if let Some(idx) = self.sessions.iter().position(|s| s.ob_id == ob_id) {
            let id = self.sessions[idx].id;
            self.sessions.remove(idx);
            SESSION_ACTIVE.fetch_and(!bit(id), Ordering::Release);
            SESSION_REGISTERED.fetch_and(!bit(id), Ordering::Release);
            true
        } else {
            false
        }
    }

    /// A session is only usable while it exists and is `Active`; unknown ids
    /// (legacy unbound tokens, session 0) are not restricted here.
    pub fn allows_token(&self, token: &Token) -> bool {
        allows_token_session(token.session_id)
    }
}

/// Authenticate `username`/`password` and bind the issued token to `session_id`.
pub fn logon(session_id: u32, username: &str, password: &[u8]) -> Result<Token, SessionError> {
    let entry = {
        let mut db = SAM_DB.lock();
        db.ensure_default_credentials();
        db.authenticate(username, password)
            .map_err(SessionError::Auth)?
            .clone()
    };
    crate::security::SESSIONS.lock().bind(session_id, &entry)
}

/// Lock-free session validity: only registered sessions can be invalid, a
/// logged-off/locked one denies its tokens, and unknown ids keep the legacy
/// (unbound) behaviour.
pub fn allows_token_session(session_id: u32) -> bool {
    if session_id == 0 || session_id as usize > MAX_SESSIONS {
        return true;
    }
    if SESSION_REGISTERED.load(Ordering::Acquire) & bit(session_id) == 0 {
        return true;
    }
    SESSION_ACTIVE.load(Ordering::Acquire) & bit(session_id) != 0
}

/// True when the token's session has not been logged off. Tokens that are not
/// bound to a registered session (session 0, or phase-1 tokens) stay valid.
pub fn token_session_valid(token: &Token) -> bool {
    allows_token_session(token.session_id)
}

/// Session id backing an Object Manager object.
pub fn id_for_ob(ob_id: u64) -> Option<u32> {
    crate::security::SESSIONS.lock().id_for_ob(ob_id)
}

/// Change a session's state (`Active -> Locked`, ...).
pub fn set_state(id: u32, state: SessionState) -> Result<(), SessionError> {
    crate::security::SESSIONS.lock().set_state(id, state)
}

/// Drop the session token and invalidate issued copies.
pub fn logoff(id: u32) -> Result<(), SessionError> {
    crate::security::SESSIONS.lock().logoff(id)
}

/// Cleanup hook for `\Session\{id}` object destruction.
pub fn destroy_session(ob_id: u64) {
    crate::security::SESSIONS.lock().remove_by_ob(ob_id);
}

pub fn register_session_tests() {
    use crate::{test_eq, test_false, test_true};
    use crate::test_case;

    test_case!("sam_password_verification", {
        let mut db = crate::security::sam::SamDatabase::new();
        let mut entry = crate::security::sam::SamEntry::new(
            "amartinper",
            crate::security::sid::sid_builtin_account(1001),
            false,
        );
        // No password set: only the empty password authenticates.
        test_true!(entry.verify_password(b""));
        test_false!(entry.verify_password(b"neodos"));

        entry.set_password(b"neodos");
        test_true!(entry.has_password());
        test_false!(entry.password_hash == [0u8; 32]);
        test_true!(entry.verify_password(b"neodos"));
        test_false!(entry.verify_password(b""));
        test_false!(entry.verify_password(b"neodos2"));

        db.add_user(entry).unwrap();
        test_true!(db.authenticate("AMARTINPER", b"neodos").is_ok());
        test_eq!(
            db.authenticate("amartinper", b"bad").err(),
            Some(SamError::BadPassword)
        );
        test_eq!(
            db.authenticate("missing", b"x").err(),
            Some(SamError::EntryNotFound)
        );
    });

    test_case!("session_lifecycle_binds_token", {
        let mut mgr = SessionManager::new();
        let id = mgr.register(None, 900, 0).unwrap();
        test_eq!(mgr.get(id).unwrap().state, SessionState::LoggedOff);

        let entry = {
            let db = SAM_DB.lock();
            db.find_by_username("NeoDOS").unwrap().clone()
        };
        let token = mgr.bind(id, &entry).unwrap();
        test_eq!(token.session_id, id);
        test_true!(mgr.allows_token(&token));
        test_eq!(mgr.get(id).unwrap().state, SessionState::Active);

        mgr.logoff(id).unwrap();
        test_false!(mgr.allows_token(&token));
        test_eq!(mgr.get(id).unwrap().state, SessionState::LoggedOff);
        test_true!(mgr.get(id).unwrap().token.is_none());

        test_true!(mgr.remove_by_ob(900));
        test_true!(mgr.get(id).is_none());
    });

    test_case!("session_logon_authenticates_sam", {
        let id = crate::security::SESSIONS
            .lock()
            .register(None, 901, 0)
            .unwrap();
        // Bind through the production path: SAM lookup + token issuance.
        let token = logon(id, "NeoDOS", b"").unwrap();
        test_eq!(token.session_id, id);
        test_false!(token.is_admin_token());

        let admin = logon(id, "Administrator", b"neodos");
        test_eq!(
            admin,
            Err(SessionError::AlreadyActive)
        );

        crate::security::SESSIONS.lock().logoff(id).unwrap();
        test_eq!(
            logon(id, "NeoDOS", b"wrong"),
            Err(SessionError::Auth(SamError::BadPassword))
        );
        test_eq!(
            logon(id, "amartinper", b"neodos").map(|t| (t.sid, t.is_admin_token())),
            Ok((crate::security::sid::sid_builtin_account(1001), false))
        );
        crate::security::SESSIONS.lock().remove_by_ob(901);
    });

    test_case!("session_logoff_denies_access", {
        use crate::security::acl::ACCESS_READ;
        use crate::security::access::se_access_check;

        let id = crate::security::SESSIONS
            .lock()
            .register(None, 902, 0)
            .unwrap();
        let entry = {
            let db = SAM_DB.lock();
            db.find_by_username("NeoDOS").unwrap().clone()
        };
        let token = crate::security::SESSIONS.lock().bind(id, &entry).unwrap();
        crate::security::SESSIONS.lock().logoff(id).unwrap();

        // A NULL descriptor would otherwise grant access; a logged-off session
        // must deny before the descriptor is consulted.
        test_false!(se_access_check(&token, None, ACCESS_READ));
        crate::security::SESSIONS.lock().remove_by_ob(902);
    });
}
