//! The words of the confirm before a lawyer's MCP server is added or what it starts changes
//! (`super::LaunchConfirm`), in English and Hindi by the app's language (`ui.language`). Their
//! review home is `ui/app/src/review/registry-uc-mcp-confirm.tsx` (UC-214 to UC-221), where the
//! Hindi is provisional; a test fails when the two disagree. `{program}` is a local server's
//! command and arguments ([`shown_program`]), `{address}` a remote server's URL
//! ([`shown_address`]), and `{names}` the new or changed variables or headers ([`shown_names`]):
//! the only parts of the dialog that come from the settings window. Nothing is ever cut: the host
//! refuses an ask too long to show whole (`MAX_ASK_CHARS`) before it reaches here.

use clauseo_host::McpLaunchAsk;
use clauseo_protocol::Language;

use crate::dialogs::words::fill;

/// One language's words for both kinds of server.
pub(super) struct Table {
    /// UC-214: the title for a local server, one that runs a program.
    pub(super) local_title: &'static str,
    /// UC-215: its message; `{program}` is what starts.
    pub(super) local_message: &'static str,
    /// UC-216: the button that allows the save.
    pub(super) ok: &'static str,
    /// UC-217: the button that saves nothing.
    pub(super) cancel: &'static str,
    /// UC-218: the title for a remote server, one reached at a URL.
    pub(super) remote_title: &'static str,
    /// UC-219: its message; `{address}` is the URL.
    pub(super) remote_message: &'static str,
    /// UC-220: the line under a local server's message when variables are new or changed;
    /// `{names}` is their names.
    pub(super) local_names: &'static str,
    /// UC-221: the same for a remote server's headers.
    pub(super) remote_names: &'static str,
}

pub(super) const EN: Table = Table {
    local_title: "Let Clauseo run this program?",
    local_message: "“{program}” will start when you work in a matter. Only allow programs you trust.",
    ok: "Allow",
    cancel: "Cancel",
    remote_title: "Let Clauseo connect to this tool?",
    remote_message: "Clauseo will connect to “{address}” when you work in a matter. Only allow \
                     tools you trust.",
    local_names: "New or changed settings: {names}.",
    remote_names: "New or changed headers: {names}.",
};

/// Provisional Hindi, for the copy review (module docs).
pub(super) const HI: Table = Table {
    local_title: "Clauseo को यह प्रोग्राम चलाने दें?",
    local_message: "जब आप किसी मामले में काम करेंगे, तब “{program}” शुरू होगा। केवल उन्हीं \
                    प्रोग्राम को अनुमति दें जिन पर आपको भरोसा है।",
    ok: "अनुमति दें",
    cancel: "रद्द करें",
    remote_title: "Clauseo को इस टूल से जुड़ने दें?",
    remote_message: "जब आप किसी मामले में काम करेंगे, तब Clauseo “{address}” से जुड़ेगा। केवल उन्हीं \
                     टूल को अनुमति दें जिन पर आपको भरोसा है।",
    local_names: "नई या बदली हुई सेटिंग्स: {names}।",
    remote_names: "नए या बदले हुए हेडर: {names}।",
};

/// The table for `language`.
pub(super) fn table(language: Language) -> &'static Table {
    match language {
        Language::En => &EN,
        Language::Hi => &HI,
    }
}

/// The confirm's words for one ask, in one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchWords {
    /// The dialog's title.
    pub(crate) title: String,
    /// Its message, naming what starts or where it connects, then, when variables or headers are
    /// new or changed, a line naming them.
    pub(crate) message: String,
    /// The button that allows the save.
    pub(crate) ok: String,
    /// The button that saves nothing.
    pub(crate) cancel: String,
}

impl LaunchWords {
    /// The words for `ask` in `language`: a remote server's when it has a URL, else a local
    /// one's. The slot is filled once ([`fill`]), so a program that spells `{address}` stays as
    /// it is.
    pub(crate) fn new(ask: &McpLaunchAsk, language: Language) -> Self {
        let table = table(language);
        let (title, mut message, names_line) = match &ask.url {
            Some(url) => (
                table.remote_title,
                fill(table.remote_message, &[("address", &shown_address(url))]),
                table.remote_names,
            ),
            None => (
                table.local_title,
                fill(
                    table.local_message,
                    &[("program", &shown_program(&ask.program))],
                ),
                table.local_names,
            ),
        };
        if !ask.names.is_empty() {
            message.push_str("\n\n");
            message.push_str(&fill(names_line, &[("names", &shown_names(&ask.names))]));
        }
        Self {
            title: title.to_owned(),
            message,
            ok: table.ok.to_owned(),
            cancel: table.cancel.to_owned(),
        }
    }
}

/// A local server's command and arguments, verbatim and whole, joined by spaces, each quoted as
/// a shell would when it is empty or holds a space or a quote ([`quoted`]).
pub(super) fn shown_program(program: &[String]) -> String {
    program
        .iter()
        .map(|part| quoted(part))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A remote server's URL, whole, its invisible characters written out.
pub(super) fn shown_address(url: &str) -> String {
    visible(url)
}

/// The new or changed names, verbatim and whole, joined by ", ", their invisible characters
/// written out.
pub(super) fn shown_names(names: &[String]) -> String {
    names
        .iter()
        .map(|name| visible(name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `part` as a shell reads it back: as it is, or in single quotes when it is empty or holds
/// whitespace or a quote; in double quotes, `"` and `\` escaped, when it holds a single quote.
/// Its invisible characters are written out ([`visible`]).
fn quoted(part: &str) -> String {
    let part = visible(part);
    let plain = !part.is_empty()
        && !part
            .chars()
            .any(|c| c.is_whitespace() || c == '\'' || c == '"');
    if plain {
        part
    } else if !part.contains('\'') {
        format!("'{part}'")
    } else {
        let escaped = part.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    }
}

/// `text` with each control character, and each that turns or hides text (bidirectional
/// overrides and isolates, zero-width marks), written as its escape (`\n`, `\u{202e}`): a
/// program can't add a line to the dialog, or show its words in another order.
fn visible(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || hides(c) {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// Whether `c` turns or hides the text around it.
fn hides(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
}

#[cfg(test)]
impl LaunchWords {
    /// The words for a local and a remote ask in every language, for tests.
    pub(crate) fn every_table_for_tests() -> Vec<Self> {
        let local = McpLaunchAsk {
            server: "files".to_owned(),
            program: vec!["npx".to_owned(), "-y".to_owned(), "files-mcp".to_owned()],
            url: None,
            names: vec!["FILES_TOKEN".to_owned()],
        };
        let remote = McpLaunchAsk {
            server: "search".to_owned(),
            program: Vec::new(),
            url: Some("https://tools.example/mcp".to_owned()),
            names: vec!["Authorization".to_owned()],
        };
        [Language::En, Language::Hi]
            .into_iter()
            .flat_map(|language| [Self::new(&local, language), Self::new(&remote, language)])
            .collect()
    }
}
