//! A PowerShell command split into simple commands, for [`super::add_script`]: the plain forms
//! an agent writes (`cd "C:\Matter"; stetdoc edit 'Main Agreement.docx' --ops ops.json`).
//!
//! `'…'` is literal (`''` a quote), `"…"` too but for `""` (a quote); a `\` is a folder
//! separator, never an escape. Commands split at `;`, `&`, `&&`, `|`, `||`, `(`, `)` and new
//! lines; `#` at a word's start comments out the rest of the line. `>` and `>>` (after `2`,
//! `*` and the like) redirect to a file, `>&1` to a stream. Anything PowerShell could expand
//! or read another way leaves its word unclear, so it writes nothing: a `$`, a backtick, a
//! glob (`*`, `?`, `[`), a brace, `@` (a splat, an array or a here-string) or a leading `~`.

use super::{Reader, Segment};

/// The simple commands of `script`, as PowerShell splits them.
pub(super) fn segments(script: &str) -> Vec<Segment> {
    let mut r = Reader::default();
    let mut chars = script.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                r.started = true;
                while let Some(q) = chars.next() {
                    if q == '\'' && chars.next_if_eq(&'\'').is_none() {
                        break;
                    }
                    r.word.push(q);
                }
            }
            '"' => {
                r.started = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' if chars.next_if_eq(&'"').is_none() => break,
                        '$' | '`' => {
                            r.unclear = true;
                            r.word.push(q);
                        }
                        _ => r.word.push(q),
                    }
                }
            }
            '#' if !r.started => while chars.next_if(|d| *d != '\n').is_some() {},
            '$' | '`' | '*' | '?' | '[' | '{' | '}' | '@' => {
                r.unclear = true;
                r.push(c);
            }
            '~' if !r.started => {
                r.unclear = true;
                r.push(c);
            }
            '>' => {
                // `2>`, `*>`: the stream before it is no word
                if r.started && !r.unclear && r.word.chars().all(|d| d.is_ascii_digit())
                    || r.word == "*"
                {
                    r.word.clear();
                    r.started = false;
                    r.unclear = false;
                }
                r.end_word();
                chars.next_if_eq(&'>');
                if chars.next_if_eq(&'&').is_some() {
                    // `2>&1`: a copy of another stream, no file
                    while chars.next_if(char::is_ascii_digit).is_some() {}
                } else {
                    r.redirect = true;
                }
            }
            ';' | '&' | '|' | '\n' | '(' | ')' | '<' => {
                if matches!(c, '&' | '|') {
                    chars.next_if_eq(&c);
                }
                r.end_segment();
            }
            c if c.is_whitespace() => r.end_word(),
            _ => r.push(c),
        }
    }
    r.end_segment();
    r.out
}
