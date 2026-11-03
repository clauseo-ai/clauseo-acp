//! Snippets as [`SnippetPart`]s: FTS5 marks each match between U+0002 and U+0003, and the
//! markers become `hit` flags, so the app never maps offsets between UTF-8 and UTF-16.
//! FTS5 marks whole tokens, so a prefix match marks the whole word.

use crate::SnippetPart;

/// The most tokens FTS5 puts in one snippet.
pub const SNIPPET_TOKENS: u8 = 32;

/// The most characters a snippet has, its final `…` included.
pub const MAX_SNIPPET_CHARS: usize = 240;

/// The parts of `raw`, cut to [`MAX_SNIPPET_CHARS`] characters.
pub(crate) fn parts(raw: &str) -> Vec<SnippetPart> {
    // The text alternates outside, inside, outside...: FTS5 never nests the markers.
    let pieces = raw
        .split(['\u{2}', '\u{3}'])
        .zip([false, true].into_iter().cycle());
    let mut out: Vec<SnippetPart> = Vec::new();
    let total = raw
        .chars()
        .filter(|c| !matches!(c, '\u{2}' | '\u{3}'))
        .count();
    if total <= MAX_SNIPPET_CHARS {
        for (piece, hit) in pieces {
            push(&mut out, piece, hit);
        }
        return out;
    }
    let chars: Vec<(char, bool)> = pieces
        .flat_map(|(piece, hit)| piece.chars().map(move |c| (c, hit)))
        .collect();
    let mut cut = MAX_SNIPPET_CHARS - 1;
    // Never leave a joiner at the end or split a mark from its base.
    while cut > 0 && (extends(chars[cut].0) || chars[cut - 1].0 == '\u{200D}') {
        cut -= 1;
    }
    let mut buffer = [0u8; 4];
    for (c, hit) in &chars[..cut] {
        push(&mut out, c.encode_utf8(&mut buffer), *hit);
    }
    push(&mut out, "…", false);
    out
}

/// Characters that extend the one before them into one visible character, as far as a cut
/// needs without Unicode tables: combining marks, Devanagari signs, the zero-width joiner,
/// variation selectors, emoji skin tones and tag characters.
fn extends(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{0900}'..='\u{0903}'
            | '\u{093A}'..='\u{093C}'
            | '\u{093E}'..='\u{094F}'
            | '\u{0951}'..='\u{0957}'
            | '\u{0962}'..='\u{0963}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{200C}'..='\u{200D}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{1F3FB}'..='\u{1F3FF}'
            | '\u{E0020}'..='\u{E007F}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

/// Appends `text`, joining it to the last part when both are hits or both are not.
fn push(out: &mut Vec<SnippetPart>, text: &str, hit: bool) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.hit == hit => last.text.push_str(text),
        _ => out.push(SnippetPart {
            text: text.to_owned(),
            hit,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SNIPPET_CHARS, parts};

    fn joined(raw: &str) -> String {
        parts(raw).iter().map(|part| part.text.as_str()).collect()
    }

    #[test]
    fn a_cut_keeps_a_joined_emoji_or_a_devanagari_syllable_whole() {
        // The cut falls at character 239: inside the family emoji, then before a vowel sign.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        let raw = format!("{}{family} tail {}", "a".repeat(236), "b".repeat(300));
        let text = joined(&raw);
        assert_eq!(text, format!("{}…", "a".repeat(236)));
        let raw = format!("{}की {}", "a".repeat(238), "b".repeat(300));
        assert_eq!(joined(&raw), format!("{}…", "a".repeat(238)));
    }

    #[test]
    fn markers_become_hit_parts() {
        let parts = parts("the \u{2}tenant\u{3} paid \u{2}rent\u{3}");
        let hits: Vec<(&str, bool)> = parts.iter().map(|p| (p.text.as_str(), p.hit)).collect();
        assert_eq!(
            hits,
            vec![
                ("the ", false),
                ("tenant", true),
                (" paid ", false),
                ("rent", true)
            ]
        );
    }

    #[test]
    fn a_long_snippet_is_cut_on_a_character_with_an_ellipsis() {
        let raw = format!("\u{2}{}\u{3} tail", "अ".repeat(500));
        let text = joined(&raw);
        assert_eq!(text.chars().count(), MAX_SNIPPET_CHARS);
        assert!(text.ends_with('…'));
        // Exactly the limit is kept whole.
        let exact = "a".repeat(MAX_SNIPPET_CHARS);
        assert_eq!(joined(&exact), exact);
    }
}
