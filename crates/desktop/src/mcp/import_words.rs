//! The words of the one confirm before Settings › Tools' import adds servers from another app
//! (`super::LaunchConfirm::confirm_import`), in English and Hindi by the app's language. Their
//! review home is `ui/app/src/review/registry-uc-mcp-import.tsx` (UC-231, UC-232, UC-336 to
//! UC-339); the buttons and the names line are A1's (UC-216, UC-217, UC-220). `{list}` names
//! every server it imports with what it starts, whole, in the host's own shown text
//! (`mcp_shown::import_list`), the text the host measured against `MAX_ASK_CHARS`: nothing in it
//! is cut. The servers that did not fit are left out whole and named on a line of their own
//! (UC-336 to UC-339), or only counted when their names would pass `MAX_LEFT_OUT_CHARS`.

use clauseo_host::{McpLaunchAsk, mcp_shown};
use clauseo_protocol::Language;

use super::LaunchWords;
use super::words::table;
use crate::dialogs::words::fill;

/// One language's words for the import's confirm.
pub(super) struct ImportTable {
    /// UC-231: the title.
    pub(super) title: &'static str,
    /// UC-232: the message; `{list}` is the servers and what each starts.
    pub(super) message: &'static str,
    /// UC-336: the line when more servers did not fit, named; `{count}` and `{names}`.
    pub(super) left_named: &'static str,
    /// UC-337: the same for one server; `{names}` is its name.
    pub(super) left_one_named: &'static str,
    /// UC-338: the line when more servers did not fit, too many to name; `{count}`.
    pub(super) left: &'static str,
    /// UC-339: the same for one server whose name is too long to show.
    pub(super) left_one: &'static str,
}

pub(super) const EN: ImportTable = ImportTable {
    title: "Let Clauseo run these programs?",
    message: "These will start when you work in a matter: {list}. Only allow programs you trust.",
    left_named: "{count} more tools didn't fit here and won't be imported now: {names}. Import \
                 again to add them.",
    left_one_named: "1 more tool didn't fit here and won't be imported now: {names}. Import again \
                     to add it.",
    left: "{count} more tools didn't fit here and won't be imported now. Import again to add them.",
    left_one: "1 more tool didn't fit here and won't be imported now. Import again to add it.",
};

/// Provisional Hindi, for the copy review (module docs).
pub(super) const HI: ImportTable = ImportTable {
    title: "Clauseo को ये प्रोग्राम चलाने दें?",
    message: "जब आप किसी मामले में काम करेंगे, तब ये शुरू होंगे: {list}। केवल उन्हीं प्रोग्राम को अनुमति \
              दें जिन पर आपको भरोसा है।",
    left_named: "{count} और टूल यहाँ नहीं आ सके और अभी इम्पोर्ट नहीं होंगे: {names}। उन्हें जोड़ने के \
                 लिए फिर से इम्पोर्ट करें।",
    left_one_named: "1 और टूल यहाँ नहीं आ सका और अभी इम्पोर्ट नहीं होगा: {names}। उसे जोड़ने के लिए \
                     फिर से इम्पोर्ट करें।",
    left: "{count} और टूल यहाँ नहीं आ सके और अभी इम्पोर्ट नहीं होंगे। उन्हें जोड़ने के लिए फिर से \
           इम्पोर्ट करें।",
    left_one: "1 और टूल यहाँ नहीं आ सका और अभी इम्पोर्ट नहीं होगा। उसे जोड़ने के लिए फिर से इम्पोर्ट \
               करें।",
};

/// The words for an import of `asks` in `language`: each server as `name (what starts)`,
/// joined by commas; then, when any has variables or headers, A1's names line (UC-220) with each
/// as `NAME (server)`; then, when servers were `left_out`, the line saying how many and, when
/// they fit, which. Each slot is filled once ([`fill`]).
pub(crate) fn import_words(
    asks: &[McpLaunchAsk],
    left_out: &[String],
    language: Language,
) -> LaunchWords {
    let words = match language {
        Language::En => &EN,
        Language::Hi => &HI,
    };
    let a1 = table(language);
    let mut message = fill(words.message, &[("list", &mcp_shown::import_list(asks))]);
    let names = mcp_shown::import_names(asks);
    if !names.is_empty() {
        message.push_str("\n\n");
        message.push_str(&fill(a1.local_names, &[("names", &names)]));
    }
    if !left_out.is_empty() {
        let count = left_out.len().to_string();
        let shown = mcp_shown::left_out(left_out);
        let template = match (left_out.len(), &shown) {
            (1, Some(_)) => words.left_one_named,
            (_, Some(_)) => words.left_named,
            (1, None) => words.left_one,
            (_, None) => words.left,
        };
        let shown = shown.unwrap_or_default();
        message.push_str("\n\n");
        message.push_str(&fill(template, &[("count", &count), ("names", &shown)]));
    }
    LaunchWords {
        title: words.title.to_owned(),
        message,
        ok: a1.ok.to_owned(),
        cancel: a1.cancel.to_owned(),
    }
}

#[cfg(test)]
#[path = "import_words_tests.rs"]
mod tests;
