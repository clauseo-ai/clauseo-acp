//! The words of the one confirm before Settings › Tools' import adds servers from another app
//! (`super::LaunchConfirm::confirm_import`), in English and Hindi by the app's language. Their
//! review home is `ui/app/src/review/registry-uc-mcp-import.tsx` (UC-231 to UC-233); the
//! buttons are A1's (UC-216, UC-217). `{list}` names each server with what it starts, shown as
//! A1's confirm shows one ([`shown_program`], [`shown_address`]).

use clauseo_host::McpLaunchAsk;
use clauseo_protocol::Language;

use super::LaunchWords;
use super::words::{shown_address, shown_program, table};
use crate::dialogs::words::fill;

/// One language's words for the import's confirm.
pub(super) struct ImportTable {
    /// UC-231: the title.
    pub(super) title: &'static str,
    /// UC-232: the message; `{list}` is the servers and what each starts.
    pub(super) message: &'static str,
    /// UC-233: the end of a list cut at [`MAX_LISTED`]; `{count}` is how many more.
    pub(super) more: &'static str,
}

pub(super) const EN: ImportTable = ImportTable {
    title: "Let Clauseo run these programs?",
    message: "These will start when you work in a matter: {list}. Only allow programs you trust.",
    more: "and {count} more",
};

/// Provisional Hindi, for the copy review (module docs).
pub(super) const HI: ImportTable = ImportTable {
    title: "Clauseo को ये प्रोग्राम चलाने दें?",
    message: "जब आप किसी मामले में काम करेंगे, तब ये शुरू होंगे: {list}। केवल उन्हीं प्रोग्राम को अनुमति \
              दें जिन पर आपको भरोसा है।",
    more: "और {count} अन्य",
};

/// The most servers the list names; the rest are counted (UC-233).
pub(super) const MAX_LISTED: usize = 8;

/// The words for an import of `asks` in `language`: each server as `name (what starts)`,
/// joined by commas, at most [`MAX_LISTED`] of them. Each slot is filled once ([`fill`]).
pub(crate) fn import_words(asks: &[McpLaunchAsk], language: Language) -> LaunchWords {
    let words = match language {
        Language::En => &EN,
        Language::Hi => &HI,
    };
    let mut items: Vec<String> = asks.iter().take(MAX_LISTED).map(item).collect();
    let rest = asks.len().saturating_sub(MAX_LISTED);
    if rest > 0 {
        items.push(fill(words.more, &[("count", &rest.to_string())]));
    }
    let buttons = table(language);
    LaunchWords {
        title: words.title.to_owned(),
        message: fill(words.message, &[("list", &items.join(", "))]),
        ok: buttons.ok.to_owned(),
        cancel: buttons.cancel.to_owned(),
    }
}

/// One server: its name (D's names are letters, digits, `_` and `-`), then what it starts.
fn item(ask: &McpLaunchAsk) -> String {
    let what = match &ask.url {
        Some(url) => shown_address(url),
        None => shown_program(&ask.program),
    };
    format!("{} ({what})", ask.server)
}

#[cfg(test)]
#[path = "import_words_tests.rs"]
mod tests;
