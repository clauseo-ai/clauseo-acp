//! What the confirm before a lawyer's MCP server starts shows, written once for both sides: the
//! desktop draws exactly these strings (`desktop/src/mcp/words.rs`, `import_words.rs`), and the
//! host measures exactly these strings against [`MAX_ASK_CHARS`] before it asks (`ask.rs`, the
//! import's fit). So the cap counts the text as the lawyer reads it, after quoting and escaping,
//! and what is allowed is never longer than what was shown.
//!
//! - A program is its command and arguments joined by spaces, each quoted as a shell reads it
//!   back ([`program`]); a URL is shown whole ([`address`]); names are joined by `, `
//!   ([`names`]). Each control character, and each that turns or hides text, is written as its
//!   escape (`\n`, `\u{202e}`), so nothing can add a line or reorder the words.
//! - Nothing here cuts text. An ask over the cap is refused (a save) or left out whole (an
//!   import's server), never shortened.

use super::McpLaunchAsk;

/// The most characters the parts of one confirm that come from a config may hold, counted as
/// shown: one save's program or URL plus its names line, or an import's list plus its names line.
pub const MAX_ASK_CHARS: usize = 2000;

/// The most characters the names of the servers an import left out may take in its confirm;
/// past it the confirm gives their count alone. A quarter of [`MAX_ASK_CHARS`]: room for about
/// 50 typical names, and the dialog stays bounded.
pub const MAX_LEFT_OUT_CHARS: usize = MAX_ASK_CHARS / 4;

/// A local server's command and arguments, verbatim and whole, joined by spaces, each quoted as
/// a shell would when it is empty or holds whitespace or a quote ([`quoted`]).
#[must_use]
pub fn program(program: &[String]) -> String {
    program
        .iter()
        .map(|part| quoted(part))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A remote server's URL, whole, its invisible characters written out.
#[must_use]
pub fn address(url: &str) -> String {
    visible(url)
}

/// Names, verbatim and whole, joined by ", ", their invisible characters written out.
#[must_use]
pub fn names(names: &[String]) -> String {
    names
        .iter()
        .map(|name| visible(name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What one ask starts: its URL, or its program.
#[must_use]
pub fn what(ask: &McpLaunchAsk) -> String {
    match &ask.url {
        Some(url) => address(url),
        None => program(&ask.program),
    }
}

/// The characters one save's confirm shows from its ask: what starts, and the names line's names.
#[must_use]
pub fn ask_chars(ask: &McpLaunchAsk) -> usize {
    what(ask).chars().count() + names(&ask.names).chars().count()
}

/// An import's list: each server as `name (what starts)`, joined by ", ".
#[must_use]
pub fn import_list(asks: &[McpLaunchAsk]) -> String {
    asks.iter()
        .map(|ask| format!("{} ({})", visible(&ask.server), what(ask)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// An import's names line's names: each variable or header as `NAME (server)`, joined by ", ";
/// empty when no server has any.
#[must_use]
pub fn import_names(asks: &[McpLaunchAsk]) -> String {
    let all: Vec<String> = asks
        .iter()
        .flat_map(|ask| {
            ask.names
                .iter()
                .map(move |name| format!("{name} ({})", ask.server))
        })
        .collect();
    names(&all)
}

/// The characters an import's confirm shows from its asks: the list and the names line's names.
#[must_use]
pub fn import_chars(asks: &[McpLaunchAsk]) -> usize {
    import_list(asks).chars().count() + import_names(asks).chars().count()
}

/// The names of the servers an import left out, as its confirm shows them, when they fit in
/// [`MAX_LEFT_OUT_CHARS`]; `None` when there are none or they do not fit (the confirm then gives
/// only their count).
#[must_use]
pub fn left_out(left: &[String]) -> Option<String> {
    let shown = names(left);
    (!left.is_empty() && shown.chars().count() <= MAX_LEFT_OUT_CHARS).then_some(shown)
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
#[path = "shown_tests.rs"]
mod tests;
