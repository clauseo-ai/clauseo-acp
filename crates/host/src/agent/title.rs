//! The agent's own name, as the header shows it: `initialize`'s `agentInfo.title`, cleaned for
//! display. It is display text only; the agent is still picked and identified by its settings id.

#[cfg(test)]
mod tests;

use clauseo_protocol::CompactString;

/// The most `char`s of an agent's name that the header shows.
const MAX_NAME_CHARS: usize = 64;

/// The agent's title as it is shown: whitespace runs (tabs and line breaks too) become one space,
/// control characters and bidi embedding, override and isolate characters are dropped, the
/// result is cut to [`MAX_NAME_CHARS`] `char`s and then has its trailing space dropped. Empty
/// after that is `None`.
pub(crate) fn clean_title(title: &str) -> Option<CompactString> {
    let mut cleaned = String::new();
    let mut space = false;
    for ch in title.chars() {
        if ch.is_whitespace() {
            space = true;
        } else if !(ch.is_control() || is_bidi_control(ch)) {
            if space && !cleaned.is_empty() {
                cleaned.push(' ');
            }
            space = false;
            cleaned.push(ch);
        }
    }
    let capped: String = cleaned.chars().take(MAX_NAME_CHARS).collect();
    let shown = capped.trim_end();
    (!shown.is_empty()).then(|| CompactString::from(shown))
}

/// The bidi embedding, override and isolate characters: U+202A to U+202E and U+2066 to U+2069.
fn is_bidi_control(ch: char) -> bool {
    matches!(ch, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}
