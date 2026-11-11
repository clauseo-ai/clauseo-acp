//! The words of the confirm before a lawyer's MCP server is added or what it starts changes
//! (`super::LaunchConfirm`), in English and Hindi by the app's language (`ui.language`). Their
//! review home is `ui/app/src/review/registry-uc-mcp-confirm.tsx` (UC-214 to UC-219), where the
//! Hindi is provisional; a test fails when the two disagree. `{program}` is a local server's
//! command and arguments ([`shown_program`]), `{address}` a remote server's URL
//! ([`shown_address`]): the only parts of the dialog that come from the settings window.

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
}

pub(super) const EN: Table = Table {
    local_title: "Let Clauseo run this program?",
    local_message: "“{program}” will start when you work in a matter. Only allow programs you trust.",
    ok: "Allow",
    cancel: "Cancel",
    remote_title: "Let Clauseo connect to this tool?",
    remote_message: "Clauseo will connect to “{address}” when you work in a matter. Only allow \
                     tools you trust.",
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
};

/// The table for `language`.
pub(super) fn table(language: Language) -> &'static Table {
    match language {
        Language::En => &EN,
        Language::Hi => &HI,
    }
}

/// The most characters `{program}` or `{address}` shows; a longer one loses its middle.
pub(super) const MAX_SHOWN: usize = 300;

/// The confirm's words for one ask, in one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchWords {
    /// The dialog's title.
    pub(crate) title: String,
    /// Its message, naming what starts or where it connects.
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
        let (title, message) = match &ask.url {
            Some(url) => (
                table.remote_title,
                fill(table.remote_message, &[("address", &shown_address(url))]),
            ),
            None => (
                table.local_title,
                fill(
                    table.local_message,
                    &[("program", &shown_program(&ask.program))],
                ),
            ),
        };
        Self {
            title: title.to_owned(),
            message,
            ok: table.ok.to_owned(),
            cancel: table.cancel.to_owned(),
        }
    }
}

/// A local server's command and arguments, verbatim, joined by spaces, each quoted as a shell
/// would when it is empty or holds a space or a quote ([`quoted`]), at most [`MAX_SHOWN`]
/// characters ([`shortened`]).
pub(super) fn shown_program(program: &[String]) -> String {
    let joined = program
        .iter()
        .map(|part| quoted(part))
        .collect::<Vec<_>>()
        .join(" ");
    shortened(&joined)
}

/// A remote server's URL, its invisible characters written out, at most [`MAX_SHOWN`]
/// characters.
pub(super) fn shown_address(url: &str) -> String {
    shortened(&visible(url))
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

/// `text`, or at most [`MAX_SHOWN`] characters of it, its middle cut to "…" so its start and
/// end both show. Cut by characters, so never inside one.
pub(super) fn shortened(text: &str) -> String {
    let count = text.chars().count();
    if count <= MAX_SHOWN {
        return text.to_owned();
    }
    let keep = MAX_SHOWN - 1;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start}…{end}")
}

#[cfg(test)]
impl LaunchWords {
    /// The words for a local and a remote ask in every language, for tests.
    pub(crate) fn every_table_for_tests() -> Vec<Self> {
        let local = McpLaunchAsk {
            server: "files".to_owned(),
            program: vec!["npx".to_owned(), "-y".to_owned(), "files-mcp".to_owned()],
            url: None,
        };
        let remote = McpLaunchAsk {
            server: "search".to_owned(),
            program: Vec::new(),
            url: Some("https://tools.example/mcp".to_owned()),
        };
        [Language::En, Language::Hi]
            .into_iter()
            .flat_map(|language| [Self::new(&local, language), Self::new(&remote, language)])
            .collect()
    }
}
