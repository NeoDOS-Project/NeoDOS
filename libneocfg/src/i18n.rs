//! Translation seam.
//!
//! NeoCfg uses the numeric-id i18n runtime already shipped in `libneodos`
//! (`i18n_get_id(u32)` / `tr_id!`), not string keys. The UI target implements
//! this trait over `libneodos::i18n`; host tests use an identity/mock table.

/// Resolves a numeric i18n message id to text.
///
/// Implementations must never panic on a missing id: return the id rendered as
/// text or a safe placeholder instead.
pub trait Translator {
    /// Resolve `id`. Returns a borrowed string valid for the lifetime of `self`.
    fn tr(&self, id: u32) -> &str;
}

/// A translator that renders every id as `"?"`.
///
/// Useful as a safe fallback when no translation table is loaded (the runtime
/// returns `"?"` on a miss), and as a base for tests.
pub struct IdentityTranslator;

impl Translator for IdentityTranslator {
    fn tr(&self, _id: u32) -> &str {
        "?"
    }
}
