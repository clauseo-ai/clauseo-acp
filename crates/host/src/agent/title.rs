//! The agent's own name, as the header shows it: `initialize`'s `agentInfo.title`, cleaned for
//! display. It is display text only; the agent is still picked and identified by its settings id.

#[cfg(test)]
mod tests;

use clauseo_protocol::CompactString;

/// The most `char`s of an agent's name that the header shows.
const MAX_NAME_CHARS: usize = 64;

/// The format characters, general category Cf, that a title drops: from Unicode 16.0's
/// `UnicodeData.txt`, as a table of inclusive ranges. It covers the bidi embedding, override and
/// isolate characters (U+202A to U+202E, U+2066 to U+2069) and the invisible tag characters.
/// The zero width joiner and non-joiner (U+200D, U+200C) stay out: scripts and emoji need them.
const FORMAT: &[(char, char)] = &[
    ('\u{AD}', '\u{AD}'),
    ('\u{600}', '\u{605}'),
    ('\u{61C}', '\u{61C}'),
    ('\u{6DD}', '\u{6DD}'),
    ('\u{70F}', '\u{70F}'),
    ('\u{890}', '\u{891}'),
    ('\u{8E2}', '\u{8E2}'),
    ('\u{180E}', '\u{180E}'),
    ('\u{200B}', '\u{200B}'),
    ('\u{200E}', '\u{200F}'),
    ('\u{202A}', '\u{202E}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206F}'),
    ('\u{FEFF}', '\u{FEFF}'),
    ('\u{FFF9}', '\u{FFFB}'),
    ('\u{110BD}', '\u{110BD}'),
    ('\u{110CD}', '\u{110CD}'),
    ('\u{13430}', '\u{1343F}'),
    ('\u{1BCA0}', '\u{1BCA3}'),
    ('\u{1D173}', '\u{1D17A}'),
    ('\u{E0001}', '\u{E0001}'),
    ('\u{E0020}', '\u{E007F}'),
];

/// The agent's title as it is shown: whitespace runs (tabs and line breaks too) become one space,
/// control characters and format characters ([`FORMAT`]) are dropped, the result is cut to
/// [`MAX_NAME_CHARS`] `char`s and then has its trailing space dropped. Empty after that is `None`.
pub(crate) fn clean_title(title: &str) -> Option<CompactString> {
    let mut cleaned = String::new();
    let mut space = false;
    for ch in title.chars() {
        if ch.is_whitespace() {
            space = true;
        } else if !(ch.is_control() || is_format(ch)) {
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

/// Whether `ch` is in [`FORMAT`]. The sources list's titles, queries and URLs use it too
/// (`sources.rs`), so every text the host shows from an agent drops the same characters.
pub(crate) fn is_format(ch: char) -> bool {
    FORMAT
        .iter()
        .any(|&(first, last)| (first..=last).contains(&ch))
}
