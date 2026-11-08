//! The paths the sweep's log line names: only those of tasks that keep the run at 56 (a hidden
//! task, a pool task not deleted, an unreadable one, a folder or key there again, a pool task
//! new to the second walk), so an administrator can find what blocks launches. The Application
//! log is readable by every local user, and a task's name may hold a user's SID (a cloud
//! client's do) or characters that disguise it, so each path is cleaned ([`clean`]): every
//! SID-shaped run is `<sid>`, every control, bidi or invisible formatting character is `?`,
//! and it is cut at [`PATH_CAP`] characters. At most [`LOGGED_CAP`] are named a run
//! ([`listed`]). Pure: no Windows call, tested on every OS.

/// The most paths one run's log line names.
pub const LOGGED_CAP: usize = 5;

/// The most characters of one path the log line keeps.
pub const PATH_CAP: usize = 260;

/// The log line's part naming `paths` (each [`clean`]ed, a path given twice named once): `""`
/// for none, else `; tasks that keep the sweep at 56: a, b` with at most [`LOGGED_CAP`], then
/// ` (+k more)`.
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

/// `path` as the log line names it: each `S-1-<digits>(-<digits>)+` run (either case of `S`)
/// is `<sid>`; then each C0, C1 or DEL control character and each bidi or invisible formatting
/// one (U+200B to U+200F, U+202A to U+202E, U+2066 to U+2069, U+061C, U+FEFF) is `?`; then
/// past [`PATH_CAP`] characters it is cut on a character and ends `…`.
pub fn clean(path: &str) -> String {
    let redacted = redact(path);
    let shown: String = redacted
        .chars()
        .map(|c| if hidden(c) { '?' } else { c })
        .collect();
    if shown.chars().count() <= PATH_CAP {
        return shown;
    }
    let mut cut: String = shown.chars().take(PATH_CAP).collect();
    cut.push('…');
    cut
}

/// Whether `c` is a control character or a bidi or invisible formatting one.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{061C}'
                | '\u{FEFF}'
        )
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

/// Where the SID-shaped run starting at `at` in `chars` ends: `S-1-`, digits, then one or more
/// groups of `-` and digits, as many as follow.
fn sid_at(chars: &[char], at: usize) -> Option<usize> {
    let starts = matches!(chars.get(at), Some('S' | 's'))
        && chars.get(at + 1) == Some(&'-')
        && chars.get(at + 2) == Some(&'1')
        && chars.get(at + 3) == Some(&'-');
    if !starts {
        return None;
    }
    let digits = |from: usize| {
        let run = chars
            .get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        (run > 0).then_some(from + run)
    };
    let mut end = digits(at + 4)?;
    let mut groups = 0;
    while chars.get(end) == Some(&'-') {
        let Some(next) = digits(end + 1) else {
            break;
        };
        end = next;
        groups += 1;
    }
    (groups > 0).then_some(end)
}
