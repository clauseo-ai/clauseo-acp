//! The paths the sweep's log line names: only those of tasks that keep the run at 56 (a hidden
//! task, a pool task not deleted, an unreadable one, a folder or key there again, a pool task
//! new to the second walk), so an administrator can find what blocks launches. The Application
//! log is readable by every local user, and a task's name may hold a user's SID (a cloud
//! client's do) or characters that disguise it, so each path is cleaned ([`clean`]): every
//! character outside printable ASCII is `?`, every SID-shaped run is `<sid>`, the path is
//! quoted, and it is cut at [`PATH_CAP`] characters. At most [`LOGGED_CAP`] are named a run
//! ([`listed`]). Pure: no Windows call, tested on every OS.

/// The most paths one run's log line names.
pub const LOGGED_CAP: usize = 5;

/// The most characters of one path the log line keeps.
pub const PATH_CAP: usize = 260;

/// The log line's part naming `paths` (each [`clean`]ed, a path given twice named once): `""`
/// for none, else `; tasks that keep the sweep at 56: "a", "b"` with at most [`LOGGED_CAP`],
/// then ` (+k more)`.
pub fn listed(paths: &[String]) -> String {
    let mut unique: Vec<&String> = Vec::new();
    for path in paths {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    if unique.is_empty() {
        return String::new();
    }
    let named: Vec<String> = unique
        .iter()
        .take(LOGGED_CAP)
        .map(|path| clean(path))
        .collect();
    let more = unique.len().saturating_sub(LOGGED_CAP);
    let more = if more > 0 {
        format!(" (+{more} more)")
    } else {
        String::new()
    };
    format!(
        "; tasks that keep the sweep at 56: {}{more}",
        named.join(", ")
    )
}

/// `path` as the log line names it: each character outside printable ASCII (U+0020 to U+007E)
/// is `?`; then each `S-1-<digits>(-<digits>)+` or `S-1-0x<hex>(-<digits>)+` run (either case
/// of `S` and `x`) is `<sid>`; then each `"` is `?`, and the text is in double quotes; past
/// [`PATH_CAP`] characters the text is cut on a character and ends `…` before the closing quote.
pub fn clean(path: &str) -> String {
    let ascii: String = path
        .chars()
        .map(|c| {
            if matches!(c, '\u{20}'..='\u{7E}') {
                c
            } else {
                '?'
            }
        })
        .collect();
    let redacted = redact(&ascii);
    let quoted: String = redacted
        .chars()
        .map(|c| if c == '"' { '?' } else { c })
        .collect();
    let mut inner: String = quoted.chars().take(PATH_CAP).collect();
    if quoted.chars().count() > PATH_CAP {
        inner.push('…');
    }
    format!("\"{inner}\"")
}

/// `text` with each SID-shaped run `<sid>`.
fn redact(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        if let Some(end) = sid_at(&chars, at) {
            out.push_str("<sid>");
            at = end;
        } else {
            out.push(c);
            at += 1;
        }
    }
    out
}

/// Where the SID-shaped run starting at `at` in `chars` ends: `S-1-`, then a decimal
/// authority, or `0x` and hex digits, then one or more groups of `-` and decimal digits, as
/// many as follow.
fn sid_at(chars: &[char], at: usize) -> Option<usize> {
    let starts = matches!(chars.get(at), Some('S' | 's'))
        && chars.get(at + 1) == Some(&'-')
        && chars.get(at + 2) == Some(&'1')
        && chars.get(at + 3) == Some(&'-');
    if !starts {
        return None;
    }
    let hex_authority =
        chars.get(at + 4) == Some(&'0') && matches!(chars.get(at + 5), Some('x' | 'X'));
    let mut end = if hex_authority {
        run(chars, at + 6, char::is_ascii_hexdigit)?
    } else {
        run(chars, at + 4, char::is_ascii_digit)?
    };
    let mut groups = 0;
    while chars.get(end) == Some(&'-') {
        let Some(next) = run(chars, end + 1, char::is_ascii_digit) else {
            break;
        };
        end = next;
        groups += 1;
    }
    (groups > 0).then_some(end)
}

/// Where the run of characters from `from` that satisfy `want` ends, if there is one.
fn run(chars: &[char], from: usize, want: impl Fn(&char) -> bool) -> Option<usize> {
    let len = chars.get(from..)?.iter().take_while(|c| want(c)).count();
    (len > 0).then_some(from + len)
}
