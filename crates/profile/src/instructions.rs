//! The tools part of the instructions (R21), and the composer later parts join (R22's legal
//! part). Claude gets the text through `_meta.systemPrompt.append`, Codex through
//! `developer_instructions`.

/// R21's tools part. It holds `{author}`, which [`Instructions::compose`] fills, and
/// `{work_folder}`, which the host fills per session.
pub const TOOLS: &str = include_str!("../assets/instructions/tools.md");

/// The token the profile fills with the author's name.
pub const AUTHOR: &str = "{author}";

/// The name tracked changes carry when the app holds no display name.
pub const DEFAULT_AUTHOR: &str = "Clauseo";

/// The most characters of an author name kept.
const AUTHOR_MAX: usize = 100;

/// The parts of the instructions, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instructions {
    parts: Vec<&'static str>,
}

impl Instructions {
    /// The tools part alone.
    pub fn tools() -> Self {
        Self { parts: vec![TOOLS] }
    }

    /// These parts with `part` after them (R22: the legal part).
    #[must_use]
    pub fn with_part(mut self, part: &'static str) -> Self {
        self.parts.push(part);
        self
    }

    /// The parts, each trimmed, joined by one blank line, with `{author}` filled by
    /// [`author_name`]`(author)`. `{work_folder}` and `{matter_notes}` stay for the host.
    pub fn compose(&self, author: &str) -> String {
        let text = self
            .parts
            .iter()
            .map(|part| part.trim())
            .collect::<Vec<_>>()
            .join("\n\n");
        let author = author_name(author);
        crate::placeholders::fill(&text, &[AUTHOR], &[(AUTHOR, &author)]).unwrap_or(text)
    }
}

/// The name as it may go inside `--author "…"` in a shell command (sh, PowerShell or cmd): only
/// letters, combining marks, digits, spaces and `.`, `,`, `'` and `-` are kept, every other
/// character is dropped (quotes of every kind, `$`, `%`, backticks, braces, bidi and other
/// control characters: a shell could act on each, and braces could make a placeholder the host
/// fills), spaces collapsed, at most 100 characters; [`DEFAULT_AUTHOR`] when nothing is left.
pub fn author_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|&c| is_author_char(c))
        .collect();
    let joined = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(AUTHOR_MAX).collect();
    let cut = cut.trim().to_owned();
    if cut.is_empty() {
        DEFAULT_AUTHOR.to_owned()
    } else {
        cut
    }
}

/// The allowlist: ASCII letters, digits, space and `.,'-`; beyond ASCII, letters, digits and
/// combining marks. No ASCII punctuation a shell reads gets through, and no Unicode quote,
/// `%`-like sign or format character (bidi controls are `Cf`, neither letters nor marks).
fn is_author_char(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | ',' | '\'' | '-');
    }
    c.is_alphabetic() || c.is_numeric() || is_mark(c)
}

/// Combining marks std's `is_alphabetic` misses: the combining blocks, Hebrew, Arabic and the
/// Indic scripts' signs (virama, nukta), which names in those scripts need.
fn is_mark(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
        | '\u{0483}'..='\u{0489}'
        | '\u{0591}'..='\u{05BD}'
        | '\u{05BF}' | '\u{05C1}' | '\u{05C2}' | '\u{05C4}' | '\u{05C5}' | '\u{05C7}'
        | '\u{0610}'..='\u{061A}'
        | '\u{064B}'..='\u{065F}'
        | '\u{0670}'
        | '\u{0900}'..='\u{0903}'
        | '\u{093A}'..='\u{094F}'
        | '\u{0951}'..='\u{0957}'
        | '\u{0962}' | '\u{0963}'
        | '\u{0981}'..='\u{0983}'
        | '\u{09BC}'..='\u{09CD}'
        | '\u{0A01}'..='\u{0A03}'
        | '\u{0A3C}'..='\u{0A4D}'
        | '\u{0A81}'..='\u{0A83}'
        | '\u{0ABC}'..='\u{0ACD}'
        | '\u{0B01}'..='\u{0B03}'
        | '\u{0B3C}'..='\u{0B4D}'
        | '\u{0B82}'
        | '\u{0BBE}'..='\u{0BCD}'
        | '\u{0C00}'..='\u{0C04}'
        | '\u{0C3C}'..='\u{0C4D}'
        | '\u{0C81}'..='\u{0C83}'
        | '\u{0CBC}'..='\u{0CCD}'
        | '\u{0D00}'..='\u{0D03}'
        | '\u{0D3B}'..='\u{0D4D}'
        | '\u{1AB0}'..='\u{1AFF}'
        | '\u{1DC0}'..='\u{1DFF}'
        | '\u{20D0}'..='\u{20FF}'
        | '\u{FE20}'..='\u{FE2F}'
    )
}

/// Words as `wc -w` counts them: runs of non-whitespace.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}
