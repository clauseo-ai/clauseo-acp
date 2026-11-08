//! The paths of the tasks that keep the run at 56 (a hidden task, a pool task not deleted, an
//! unreadable one, a folder or key there again, a pool task new to the second walk), so an
//! administrator can find what blocks launches. The Application log, where the sweep's line
//! goes, is readable by every local user, so they go only to the admin-only log
//! ([`Sweeper::admin_log`]: [`ADMIN_LOG_NAME`] in `%ProgramData%\Clauseo\Sandbox`, made with
//! [`ADMIN_LOG_SDDL`], capped at [`ADMIN_LOG_CAP`] and rotated once to `.1`); the line keeps
//! their count ([`to_admin_log`]). A task's name may hold a user's SID (a cloud client's do)
//! or characters that disguise it, so each path is still cleaned ([`clean`]): every character
//! outside printable ASCII is `?`, every SID-shaped run is `<sid>`, the path is quoted, and it
//! is cut at [`PATH_CAP`] characters. At most [`LOGGED_CAP`] are named a run ([`listed`]).
//! Pure but for that one call: tested on every OS.

use super::Sweeper;

/// The admin-only log's owner and DACL, given when the file is made: SYSTEM, and full control
/// for SYSTEM and Administrators alone, protected.
pub const ADMIN_LOG_SDDL: &str = "O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)";

/// The folders below `%ProgramData%` the admin-only log lives in, each with what it is made
/// with when missing; one there already must let only SYSTEM, Administrators or
/// `TrustedInstaller` change it (`admin_copy::level_safe`), or nothing is written.
pub const ADMIN_LOG_FOLDERS: [(&str, &str); 2] = [
    ("Clauseo", CLAUSEO_FOLDER_SDDL),
    ("Sandbox", ADMIN_FOLDER_SDDL),
];

/// `%ProgramData%\Clauseo` as the installer makes it (`crates/desktop/windows/hooks.nsh`, the
/// policy folder's `ConvertStringSecurityDescriptorToSecurityDescriptorW`; keep the two the
/// same): Users may read it, since the app reads a firm's `policy.json` there.
pub const CLAUSEO_FOLDER_SDDL: &str =
    "O:BAG:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";

/// What `Clauseo\Sandbox` is made with: SYSTEM and Administrators alone.
pub const ADMIN_FOLDER_SDDL: &str = "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// The admin-only log's file name; rotated once to `sweep-admin.log.1`.
pub const ADMIN_LOG_NAME: &str = "sweep-admin.log";

/// The admin-only log's cap: a line that would take it past this first moves it to `.1`.
pub const ADMIN_LOG_CAP: u64 = 256 * 1024;

/// Whether a log of `size` bytes is rotated before `add` more are appended.
pub fn rotates(size: u64, add: u64) -> bool {
    size > 0 && size.saturating_add(add) > ADMIN_LOG_CAP
}

/// Writes the paths of the tasks that keep the run at 56 to `fs`'s admin-only log, and returns
/// what the world-readable line says of them: `""` for none, else their count and whether the
/// admin-only log holds them, never a path.
pub(super) fn to_admin_log<S: Sweeper>(fs: &mut S, paths: &[String]) -> String {
    let named = listed(paths);
    if named.is_empty() {
        return String::new();
    }
    let mut unique: Vec<&String> = paths.iter().collect();
    unique.sort_unstable();
    unique.dedup();
    let count = unique.len();
    match fs.admin_log(&format!("the sandbox's task sweep{named}")) {
        Ok(()) => format!("; {count} tasks keep it at 56, named in the admin-only log"),
        Err(why) => format!(
            "; {count} tasks keep it at 56, not named: the admin-only log couldn't be written \
             ({why})"
        ),
    }
}

/// Whether the self-relative descriptor `bytes` keeps a file to SYSTEM and Administrators: one
/// of them its owner, a protected DACL, and only allow entries, each naming one of them.
#[cfg(any(windows, test))]
pub fn admin_only(bytes: &[u8]) -> bool {
    use crate::windows::acl_bytes::{ALLOWED, Acl, Descriptor, kind, owner_sid, sid, sid_text};

    const ADMINS: [&str; 2] = ["S-1-5-18", "S-1-5-32-544"];
    let owner = owner_sid(bytes);
    if !owner.is_some_and(|owner| ADMINS.contains(&owner.as_str())) {
        return false;
    }
    let Ok(descriptor) = Descriptor::parse(bytes) else {
        return false;
    };
    let Some(dacl) = descriptor.dacl.filter(|_| descriptor.dacl_protected()) else {
        return false;
    };
    let Ok(dacl) = Acl::parse(dacl) else {
        return false;
    };
    dacl.aces().iter().all(|ace| {
        let named = sid(ace).and_then(sid_text);
        kind(ace) == ALLOWED && named.is_some_and(|sid| ADMINS.contains(&sid.as_str()))
    })
}

/// The most paths one run's log line names.
pub const LOGGED_CAP: usize = 5;

/// The most characters of one path the log line keeps.
pub const PATH_CAP: usize = 260;

/// The admin-only log line's part naming `paths` (each [`clean`]ed, a path given twice named
/// once): `""` for none, else `; tasks that keep the sweep at 56: "a", "b"` with at most
/// [`LOGGED_CAP`], then ` (+k more)`.
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
