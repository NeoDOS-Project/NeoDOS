//! Certificates module — trust anchors (system bundle + imported CAs).
//!
//! Read/write but deliberately small: the system bundle is read-only, so the
//! module can only list it (with its count) and the user can import or delete
//! *imported* CAs. Validation and storage live in `libtrust`; the TUI only
//! presents what the platform seam returns.

use alloc::{boxed::Box, format, string::String, string::ToString, vec, vec::Vec};

use crate::i18n_keys as k;
use crate::model::{Field, FieldValue, Intent, MenuItem, Text, View};
use crate::module::{CfgModule, ModuleId, ModuleSession, Transition};
use crate::platform::{CertEntry, CertImport, CertOps, CertRejection, CfgPlatform};

/// Trust store module.
pub struct CertificatesModule;

impl CfgModule for CertificatesModule {
    fn id(&self) -> ModuleId {
        ModuleId::Certificates
    }

    fn title_key(&self) -> u32 {
        k::CERT_MODULE_NAME
    }

    fn description_key(&self) -> u32 {
        k::CERT_MODULE_DESC
    }

    fn create(&self) -> Box<dyn ModuleSession> {
        Box::new(CertificatesSession::new())
    }
}

/// Which screen of the module is showing.
enum Screen {
    Menu,
    Users,
    System,
    /// Detail of the CA with this fingerprint (user or system).
    Detail(String),
    /// Deletion confirmation for a user CA.
    Confirm(String),
    /// Path input for an import.
    Import,
    /// Outcome of the last import/removal.
    Result(Vec<Text>),
}

struct CertificatesSession {
    screen: Screen,
    selected: usize,
    input: String,
}

impl CertificatesSession {
    fn new() -> Self {
        CertificatesSession {
            screen: Screen::Menu,
            selected: 0,
            input: String::new(),
        }
    }

    fn menu(&self) -> Vec<MenuItem> {
        vec![
            MenuItem::new(Text::Key(k::CERT_MENU_USERS)),
            MenuItem::new(Text::Key(k::CERT_MENU_SYSTEM)),
            MenuItem::new(Text::Key(k::CERT_MENU_IMPORT)),
        ]
    }
}

fn ops(env: &dyn CfgPlatform) -> Option<&dyn CertOps> {
    env.certificates()
}

fn user_entries(ops: &dyn CertOps) -> Vec<CertEntry> {
    ops.entries().into_iter().filter(|e| !e.system).collect()
}

fn find(ops: &dyn CertOps, fingerprint: &str) -> Option<CertEntry> {
    ops.entries()
        .into_iter()
        .find(|e| e.fingerprint == fingerprint)
}

/// `YYYY-MM-DD` from Unix seconds (proleptic Gregorian, UTC).
fn format_date(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn entry_fields(entry: &CertEntry) -> Vec<Field> {
    vec![
        Field::new(
            Text::Key(k::CERT_FIELD_SUBJECT),
            FieldValue::Text(Text::Owned(entry.subject.clone())),
        ),
        Field::new(
            Text::Key(k::CERT_FIELD_ISSUER),
            FieldValue::Text(Text::Owned(entry.issuer.clone())),
        ),
        Field::new(
            Text::Key(k::CERT_FIELD_VALID),
            FieldValue::Text(Text::Owned(format!(
                "{} .. {}",
                format_date(entry.not_before),
                format_date(entry.not_after)
            ))),
        ),
        Field::new(
            Text::Key(k::CERT_FIELD_TYPE),
            FieldValue::Text(Text::Key(if entry.system {
                k::CERT_TYPE_SYSTEM
            } else {
                k::CERT_TYPE_USER
            })),
        ),
        Field::new(
            Text::Key(k::CERT_FIELD_FINGERPRINT),
            FieldValue::Text(Text::Owned(entry.fingerprint.clone())),
        ),
    ]
}

fn rejection_key(reason: CertRejection) -> u32 {
    match reason {
        CertRejection::Empty => k::CERT_REJ_EMPTY,
        CertRejection::Malformed => k::CERT_REJ_MALFORMED,
        CertRejection::NotCa => k::CERT_REJ_NOT_CA,
        CertRejection::KeyUsage => k::CERT_REJ_KEYUSAGE,
        CertRejection::Expired => k::CERT_REJ_EXPIRED,
        CertRejection::NotYetValid => k::CERT_REJ_NOT_YET,
        CertRejection::Unsupported => k::CERT_REJ_UNSUPPORTED,
        CertRejection::Io => k::CERT_REJ_IO,
    }
}

/// Flatten the per-certificate import outcomes into message lines.
fn import_lines(outcomes: &[CertImport]) -> Vec<Text> {
    let mut lines = Vec::new();
    for outcome in outcomes {
        let (fingerprint, status) = match outcome {
            CertImport::Installed(fp) => (Some(fp.as_str()), k::CERT_IMPORT_OK),
            CertImport::AlreadyUser(fp) => (Some(fp.as_str()), k::CERT_IMPORT_PRESENT),
            CertImport::AlreadySystem(fp) => (Some(fp.as_str()), k::CERT_IMPORT_PRESENT),
            CertImport::Rejected(reason) => (None, rejection_key(*reason)),
        };
        if let Some(fp) = fingerprint {
            lines.push(Text::Owned(format!("#{}  ", &fp[..fp.len().min(16)])));
        }
        lines.push(Text::Key(status));
    }
    lines.push(Text::Key(k::PRESS_KEY));
    lines
}

impl ModuleSession for CertificatesSession {
    fn view(&self, env: &dyn CfgPlatform) -> View {
        let Some(ops) = ops(env) else {
            return View::Message {
                title: Text::Key(k::CERT_TITLE),
                body: vec![Text::Key(k::CERT_UNAVAILABLE), Text::Key(k::PRESS_KEY)],
            };
        };

        match &self.screen {
            Screen::Menu => View::Menu {
                title: Text::Key(k::CERT_TITLE),
                items: self.menu(),
                selected: self.selected,
                footer: Some(Text::Key(k::NEOCFG_BACK)),
            },
            Screen::Users => {
                let users = user_entries(ops);
                if users.is_empty() {
                    return View::Message {
                        title: Text::Key(k::CERT_MENU_USERS),
                        body: vec![Text::Key(k::CERT_NO_USER), Text::Key(k::PRESS_KEY)],
                    };
                }
                let items = users
                    .iter()
                    .map(|e| {
                        MenuItem::new(Text::Owned(format!(
                            "#{}  {}",
                            &e.fingerprint[..e.fingerprint.len().min(8)],
                            e.subject
                        )))
                    })
                    .collect();
                View::Menu {
                    title: Text::Key(k::CERT_MENU_USERS),
                    items,
                    selected: self.selected,
                    footer: Some(Text::Key(k::NEOCFG_BACK)),
                }
            }
            Screen::System => {
                let system = ops.entries().into_iter().filter(|e| e.system).count();
                View::Detail {
                    title: Text::Key(k::CERT_MENU_SYSTEM),
                    fields: vec![
                        Field::new(Text::Key(k::CERT_SYSTEM_COUNT), FieldValue::Count(system as u64)),
                        Field::new(
                            Text::Key(k::CERT_FIELD_TYPE),
                            FieldValue::Text(Text::Key(k::CERT_TYPE_SYSTEM)),
                        ),
                        Field::new(
                            Text::Key(k::CERT_SYSTEM_NOTE),
                            FieldValue::Text(Text::Key(k::CERT_SYSTEM_READONLY)),
                        ),
                    ],
                    footer: Text::Key(k::NEOCFG_BACK),
                }
            }
            Screen::Detail(fp) => match find(ops, fp) {
                Some(entry) if !entry.system => View::Detail {
                    title: Text::Key(k::CERT_TITLE),
                    fields: entry_fields(&entry),
                    footer: Text::Key(k::CERT_DETAIL_FOOTER),
                },
                _ => View::Message {
                    title: Text::Key(k::CERT_TITLE),
                    body: vec![Text::Key(k::CERT_NO_USER), Text::Key(k::PRESS_KEY)],
                },
            },
            Screen::Confirm(_) => View::Confirm {
                title: Text::Key(k::CERT_CONFIRM_TITLE),
                prompt: Text::Key(k::CERT_CONFIRM_PROMPT),
                default_yes: false,
            },
            Screen::Import => View::Input {
                title: Text::Key(k::CERT_IMPORT_TITLE),
                prompt: Text::Key(k::CERT_IMPORT_PROMPT),
                buf: self.input.clone(),
                mask: false,
            },
            Screen::Result(lines) => View::Message {
                title: Text::Key(k::CERT_IMPORT_TITLE),
                body: lines.clone(),
            },
        }
    }

    fn update(&mut self, env: &dyn CfgPlatform, intent: Intent) -> Transition {
        // `Tick` is a repaint request from the UI: never change screens.
        if intent == Intent::Tick {
            return Transition::Stay;
        }
        let Some(ops) = ops(env) else {
            return Transition::Back;
        };

        match &self.screen {
            Screen::Menu => match intent {
                Intent::Select(0) => {
                    self.screen = Screen::Users;
                    self.selected = 0;
                    Transition::Stay
                }
                Intent::Select(1) => {
                    self.screen = Screen::System;
                    Transition::Stay
                }
                Intent::Select(2) => {
                    self.screen = Screen::Import;
                    self.input.clear();
                    Transition::Stay
                }
                Intent::Back => Transition::Back,
                _ => Transition::Stay,
            },
            Screen::Users => {
                let users = user_entries(ops);
                match intent {
                    Intent::Select(i) if i < users.len() => {
                        self.screen = Screen::Detail(users[i].fingerprint.clone());
                        Transition::Stay
                    }
                    Intent::Back => {
                        self.screen = Screen::Menu;
                        Transition::Stay
                    }
                    _ => Transition::Stay,
                }
            }
            Screen::System => {
                self.screen = Screen::Menu;
                Transition::Stay
            }
            Screen::Detail(fp) => match intent {
                Intent::Char('d') | Intent::Char('D') => {
                    self.screen = Screen::Confirm(fp.clone());
                    Transition::Stay
                }
                Intent::Back | Intent::Activate => {
                    self.screen = Screen::Users;
                    Transition::Stay
                }
                _ => Transition::Stay,
            },
            Screen::Confirm(fp) => match intent {
                Intent::Activate => {
                    let lines = match ops.remove(fp) {
                        Ok(()) => vec![Text::Key(k::CERT_REMOVED), Text::Key(k::PRESS_KEY)],
                        Err(_) => vec![Text::Key(k::CERT_REMOVE_FAILED), Text::Key(k::PRESS_KEY)],
                    };
                    self.screen = Screen::Result(lines);
                    Transition::Stay
                }
                _ => {
                    self.screen = Screen::Detail(fp.clone());
                    Transition::Stay
                }
            },
            Screen::Import => match intent {
                Intent::Text(path) => {
                    let path = path.trim().to_string();
                    if path.is_empty() {
                        self.screen = Screen::Menu;
                        return Transition::Stay;
                    }
                    let outcomes = ops.import_file(&path);
                    self.screen = Screen::Result(import_lines(&outcomes));
                    Transition::Stay
                }
                _ => {
                    self.screen = Screen::Menu;
                    Transition::Stay
                }
            },
            Screen::Result(_) => {
                self.screen = Screen::Users;
                Transition::Stay
            }
        }
    }
}
