//! `Translator` seam over `libneodos::i18n` (numeric ids).

use libneocfg::Translator;
use libneodos::i18n;

/// Resolves NeoCfg message ids through the runtime NLT tables.
#[derive(Clone, Copy)]
pub struct NeodosTranslator;

impl Translator for NeodosTranslator {
    fn tr(&self, id: u32) -> &str {
        i18n::i18n_get_id(id)
    }
}
