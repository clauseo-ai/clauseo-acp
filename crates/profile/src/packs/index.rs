//! Codex's skills index with the packs (decision record 0040, amending 0031's budget): the
//! built-in lines first, exactly as 0031 renders them, then one line per pack skill this
//! launch wrote:
//!
//! ```text
//! - <pack>--<skill> (<description, cut to 24 words and 200 characters>): `<path of its SKILL.md>`
//! ```
//!
//! A description that holds a control or format character, a line or paragraph separator, or a
//! placeholder the host fills is replaced by no use text (0031's path filter). A path that
//! fails that filter leaves its line out. Lines stop at [`INDEX_ENTRIES`] entries,
//! [`INDEX_WORDS`] words or [`INDEX_CHARS`] characters for the whole index, heading included;
//! the built-in lines come first and are never cut. Each line dropped is counted in one log
//! line. Pack lines need this launch's [`PacksWritten`], as built-in lines need
//! `SkillsWritten`.

use std::path::Path;

use super::plugin::PacksWritten;
use crate::skills::builtin::{code_span, index_lines, index_text, is_listable};
use crate::skills::{Rendered, SKILL_MD};

/// The most entries the index lists, built-ins included.
pub const INDEX_ENTRIES: usize = 40;

/// The most words the index has, heading and built-ins included (split on whitespace).
pub const INDEX_WORDS: usize = 600;

/// The most characters the index has, heading and built-ins included, so text with no
/// whitespace cannot pass the word budget.
pub const INDEX_CHARS: usize = 4_800;

/// The most words of a pack skill's description the index keeps.
pub const DESCRIPTION_WORDS: usize = 24;

/// The most characters of a pack skill's description the index keeps, `…` included.
pub const DESCRIPTION_CHARS: usize = 200;

/// The index: the built-in lines when `builtin` is given (the Claude plugin's `skills/` and
/// what it holds, from a successful `write_skills`), then the pack lines when `packs` is given.
/// Empty when it lists nothing.
pub fn codex_index_with_packs(
    builtin: Option<(&Path, &Rendered)>,
    packs: Option<&PacksWritten>,
) -> String {
    let mut lines = builtin.map_or_else(Vec::new, |(root, rendered)| index_lines(root, rendered));
    let Some(packs) = packs else {
        return index_text(&lines);
    };
    let mut dropped = 0usize;
    let root = packs.skills_root();
    for skill in packs.skills() {
        let path = root.join(&skill.name).join(SKILL_MD);
        let Some(path) = path.to_str().filter(|path| is_listable(path)) else {
            tracing::warn!("a pack skill's path cannot go in Codex's index; it is left out");
            continue;
        };
        let line = match use_text(&skill.description) {
            Some(text) => format!("- {} ({text}): {}", skill.name, code_span(path)),
            None => format!("- {}: {}", skill.name, code_span(path)),
        };
        if lines.len() >= INDEX_ENTRIES {
            dropped += 1;
            continue;
        }
        // The whole index with this line, heading and built-ins included: past a budget, the
        // line goes and the built-ins stay whole.
        lines.push(line);
        let text = index_text(&lines);
        if text.split_whitespace().count() > INDEX_WORDS || text.chars().count() > INDEX_CHARS {
            lines.pop();
            dropped += 1;
        }
    }
    if dropped > 0 {
        tracing::warn!(
            dropped,
            "pack skills past the index's budget are left out of it"
        );
    }
    index_text(&lines)
}

/// `description` as an index line's use text: none when it fails 0031's filter, else its first
/// [`DESCRIPTION_WORDS`] words, then at most [`DESCRIPTION_CHARS`] characters, `…` included,
/// with `…` when cut.
fn use_text(description: &str) -> Option<String> {
    if !is_listable(description) {
        return None;
    }
    let words: Vec<&str> = description.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let mut text = words
        .iter()
        .take(DESCRIPTION_WORDS)
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    let mut cut = words.len() > DESCRIPTION_WORDS;
    if text.chars().count() > DESCRIPTION_CHARS
        || (cut && text.chars().count() >= DESCRIPTION_CHARS)
    {
        // A character boundary, then no trailing space before the `…`.
        if let Some((at, _)) = text.char_indices().nth(DESCRIPTION_CHARS - 1) {
            text.truncate(at);
        }
        text.truncate(text.trim_end().len());
        cut = true;
    }
    if cut {
        text.push('…');
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::super::plugin::WrittenSkill;
    use super::*;

    /// A token for `count` skills described by `description`, under a short plugin path.
    fn written(count: usize, description: &str) -> PacksWritten {
        let skills = (0..count)
            .map(|n| WrittenSkill {
                name: format!("kit-{}--s{}", n / 32, n % 32),
                description: description.to_owned(),
            })
            .collect();
        PacksWritten::for_test(Path::new("/r"), skills)
    }

    #[test]
    fn the_entry_cap_binds_with_short_paths() {
        let index = codex_index_with_packs(None, Some(&written(64, "Short.")));
        assert_eq!(
            index.lines().filter(|line| line.starts_with("- ")).count(),
            INDEX_ENTRIES
        );
        assert!(index.chars().count() <= INDEX_CHARS);
    }

    #[test]
    fn a_description_is_cut_to_its_words_and_characters() {
        assert_eq!(use_text("Short."), Some("Short.".to_owned()));
        let words = vec!["word"; 30].join(" ");
        assert_eq!(
            use_text(&words),
            Some(format!("{}…", vec!["word"; 24].join(" ")))
        );
        // Multi-byte characters are cut at a character boundary.
        let wide = "é".repeat(250);
        let cut = use_text(&wide).unwrap();
        assert_eq!(cut.chars().count(), DESCRIPTION_CHARS);
        assert_eq!(cut, format!("{}…", "é".repeat(DESCRIPTION_CHARS - 1)));
        assert_eq!(
            use_text(&"é".repeat(DESCRIPTION_CHARS)),
            Some("é".repeat(DESCRIPTION_CHARS))
        );
    }
}
