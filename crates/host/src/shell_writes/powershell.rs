//! A PowerShell command split into simple commands, for [`super::add_input`]: the plain forms
//! an agent writes (`cd "C:\Matter"; stetdoc edit 'Main Agreement.docx' --ops ops.json`).
//!
//! Read by the reader every PowerShell reader in the host shares (`pre_write_guard`'s
//! `shell_segments`, PowerShell's own quoting: `''` and `""`, the typographic quotes, the
//! backtick's escapes, here-strings), then taken as this module needs. Commands split at `;`,
//! `&`, `&&`, `|`, `||`, `(`, `)` and new lines; `#` at a word's start comments out the rest of
//! the line. `>` and `>>` (after `2`, `*` and the like) redirect to a file, `>&1` to a stream.
//! Anything PowerShell could expand or read another way leaves its word unclear, so it writes
//! nothing: an expansion (`$name`, `$(…)`, bare or in double quotes), an unquoted glob (`*`,
//! `?`, `[`), brace or `@` (a splat or an array), a leading unquoted `~`, an array's `,`, or a
//! quote that never closes.

use crate::pre_write_guard::{ARRAY_COMMA, Escape, Joint, Role, Segments, shell_segments};

use super::{Segment, Sep, Word};

/// The simple commands of `script`, as PowerShell splits them.
pub(super) fn segments(script: &str) -> Vec<Segment> {
    let mut read = Segments::new(usize::MAX);
    shell_segments(script, Escape::Backtick, &mut read);
    // A quote, here-string or `$(…)` that never closes, or a redirection with no target: a
    // parse error, so PowerShell runs none of it. A reading cut short (`Segments::cut`): what
    // it writes is not known, so it is credited nothing either.
    if read.unsure() {
        return Vec::new();
    }
    read.iter()
        .map(|segment| {
            let mut out = Segment {
                ender: sep(segment.ender),
                nested: segment.nested,
                ..Segment::default()
            };
            let known = segment.quoted.iter().zip(&segment.unknown);
            // The targets, in the order of `redirects`: a duplication (`2>&1`) there as `&1`.
            let mut redirects = segment.redirects.iter();
            for ((text, (quoted, unknown)), role) in
                segment.words.iter().zip(known).zip(&segment.roles)
            {
                let word = Word {
                    text: text.clone(),
                    clear: clear(text, *quoted, *unknown),
                };
                match role {
                    Role::Arg => out.words.push(word),
                    Role::Target => {
                        let stream = redirects
                            .next()
                            .is_none_or(|target| target.starts_with('&'));
                        if !stream && !text.is_empty() {
                            out.redirects.push(word);
                        }
                    }
                    // A 5.1 spelling is no position: `cp`'s last argument stays the last.
                    Role::Stream | Role::Alt => {}
                }
            }
            out
        })
        .filter(|segment| !segment.words.is_empty() || !segment.redirects.is_empty())
        .collect()
}

/// [`Sep`] for the shared reader's [`Joint`].
fn sep(joint: Joint) -> Sep {
    match joint {
        Joint::Then => Sep::Then,
        Joint::And => Sep::And,
        Joint::Or => Sep::Or,
        Joint::Pipe => Sep::Pipe,
        Joint::Background => Sep::Background,
        Joint::Open => Sep::Open,
        Joint::Close => Sep::Close,
        Joint::End => Sep::End,
    }
}

/// Whether the word `text` (with any part `quoted`, holding an expansion when `unknown`) is
/// plain text (the module doc).
fn clear(text: &str, quoted: bool, unknown: bool) -> bool {
    let bare_special =
        !quoted && (text.contains(['*', '?', '[', '{', '}', '@']) || text.starts_with('~'));
    !(unknown || bare_special || text.contains(ARRAY_COMMA))
}
