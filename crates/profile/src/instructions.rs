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

/// The name as it may go inside `--author "…"` in a shell command: without `"`, `\`, `` ` ``,
/// `$`, `!`, braces and control characters (a person's name needs none of them; a shell could
/// act on each, and braces could make a placeholder the host fills), spaces collapsed, at most
/// 100 characters; [`DEFAULT_AUTHOR`] when nothing is left.
pub fn author_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '"' | '\\' | '`' | '$' | '!' | '{' | '}'))
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

/// Words as `wc -w` counts them: runs of non-whitespace.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}
