//! The words of the one confirm before Settings › Tools' import adds servers from another app
//! (`super::LaunchConfirm::confirm_import`), in English and Hindi by the app's language. Their
//! review home is `ui/app/src/review/registry-uc-mcp-import.tsx` (UC-231, UC-232); the
//! buttons and the names line are A1's (UC-216, UC-217, UC-220). `{list}` names every server
//! with what it starts, whole, as A1's confirm shows one ([`shown_program`], [`shown_address`]):
//! the host refuses an import too long to show whole (`mcp.import_too_large`) before it gets
//! here, so nothing is cut.

use clauseo_host::McpLaunchAsk;
use clauseo_protocol::Language;

use super::LaunchWords;
use super::words::{shown_address, shown_names, shown_program, table};
use crate::dialogs::words::fill;

/// One language's words for the import's confirm.
pub(super) struct ImportTable {
    /// UC-231: the title.
    pub(super) title: &'static str,
    /// UC-232: the message; `{list}` is the servers and what each starts.
    pub(super) message: &'static str,
}

pub(super) const EN: ImportTable = ImportTable {
    title: "Let Clauseo run these programs?",
    message: "These will start when you work in a matter: {list}. Only allow programs you trust.",
};

/// Provisional Hindi, for the copy review (module docs).
pub(super) const HI: ImportTable = ImportTable {
    title: "Clauseo को ये प्रोग्राम चलाने दें?",
    message: "जब आप किसी मामले में काम करेंगे, तब ये शुरू होंगे: {list}। केवल उन्हीं प्रोग्राम को अनुमति \
              दें जिन पर आपको भरोसा है।",
};

/// The words for an import of `asks` in `language`: each server as `name (what starts)`,
/// joined by commas; then, when any has variables or headers, A1's names line (UC-220) with each
/// as `NAME (server)`. Each slot is filled once ([`fill`]).
pub(crate) fn import_words(asks: &[McpLaunchAsk], language: Language) -> LaunchWords {
    let words = match language {
        Language::En => &EN,
        Language::Hi => &HI,
    };
    let items: Vec<String> = asks.iter().map(item).collect();
    let a1 = table(language);
    let mut message = fill(words.message, &[("list", &items.join(", "))]);
    let names: Vec<String> = asks
        .iter()
        .flat_map(|ask| {
            ask.names
                .iter()
                .map(move |name| format!("{name} ({})", ask.server))
        })
        .collect();
    if !names.is_empty() {
        message.push_str("\n\n");
        message.push_str(&fill(a1.local_names, &[("names", &shown_names(&names))]));
    }
    LaunchWords {
        title: words.title.to_owned(),
        message,
        ok: a1.ok.to_owned(),
        cancel: a1.cancel.to_owned(),
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
