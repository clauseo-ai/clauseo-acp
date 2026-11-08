//! An agent-chosen name as Clauseo shows it to the lawyer: in a notice
//! ([`SetAsideWhat::notice_by`](crate::SetAsideWhat::notice_by)) and in the denial pipe's
//! set-aside records (`windows::denial_pipe`). The agent picks the name, so it could hold a
//! character that makes the words read as something else: a bidi override that flips a path, a
//! newline that starts a fake sentence. Those are shown escaped, and a name is capped, so no
//! name can bury the rest of a notice.

use std::path::Path;

/// The most characters [`shown_name`] gives, its `…` included.
pub(crate) const SHOWN_CAP: usize = 512;

/// `name` as the lawyer is shown it: lossy UTF-8, each character [`is_hidden`] counts written
/// as `\u{XXXX}`, and when longer than [`SHOWN_CAP`] characters, cut to fit with `…` at the
/// end. An escape is never cut in two.
pub(crate) fn shown_name(name: &Path) -> String {
    let pieces: Vec<String> = name
        .to_string_lossy()
        .chars()
        .map(|c| {
            if is_hidden(c) {
                format!("\\u{{{:X}}}", u32::from(c))
            } else {
                c.to_string()
            }
        })
        .collect();
    let len = |piece: &String| piece.chars().count();
    if pieces.iter().map(len).sum::<usize>() <= SHOWN_CAP {
        return pieces.concat();
    }
    let mut shown = String::new();
    let mut count = 0;
    for piece in &pieces {
        if count + len(piece) > SHOWN_CAP - 1 {
            break;
        }
        shown.push_str(piece);
        count += len(piece);
    }
    shown.push('…');
    shown
}

/// Whether `c` can change how the text around it reads, or can't be seen: a control (C0, DEL,
/// C1), a line or paragraph separator (U+2028, U+2029), a bidi embedding, override or isolate
/// (U+202A to U+202E, U+2066 to U+2069), or a bidi mark (U+200E, U+200F, U+061C).
pub(crate) fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
        )
}
