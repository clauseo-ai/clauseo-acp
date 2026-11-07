//! The refresh's fixed values and pure rules: the task's argument and limit, the descriptors of
//! the task, the staging folder and the admin copy, and the comparisons of paths, descriptors
//! and the install folder against the owner's profile. No Windows call; tested on every OS.

use std::path::Path;

use crate::windows::sids::Sid;

/// The task's one argument: the action is the admin copy's path with this, nothing else.
pub const REFRESH_ARG: &str = "--refresh";
/// The task's execution limit: 5 minutes.
pub const EXECUTION_LIMIT: &str = "PT5M";

/// The descriptor of the task and its folder: protected; SYSTEM and Administrators full
/// control; the record owner `owner` read and run (`FRFX`), so the app can ask for a run and
/// can't change or delete the task.
pub fn task_sddl(owner: &Sid) -> String {
    format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;{owner})")
}

/// The staging folder's owner and DACL: SYSTEM, alone, full control, inherited by what is made
/// inside; protected.
pub const STAGING_SDDL: &str = "O:SYD:P(A;OICI;FA;;;SY)";
/// The admin copy's owner and DACL, given to the staged file before it moves into place:
/// Administrators own it; SYSTEM and Administrators have full control, Users read and run it;
/// protected. The installer's first copy (PR 1) is to carry the same, so a refresh changes no
/// access.
pub const ADMIN_COPY_SDDL: &str = "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;BU)";

/// Whether `found`, an owner and DACL read back as SDDL, is `expected`: the same text but for
/// the DACL's `AI` and `AR` flags, which Windows adds when it sets a descriptor through
/// `SetSecurityInfo` and which grant nothing.
pub fn sddl_matches(found: &str, expected: &str) -> bool {
    let plain = |sddl: &str| match sddl.split_once("D:") {
        Some((owner, dacl)) => {
            let flags = dacl.find('(').unwrap_or(dacl.len());
            let (flags, aces) = dacl.split_at(flags);
            let protected = if flags.contains('P') { "P" } else { "" };
            format!("{owner}D:{protected}{aces}")
        }
        None => sddl.to_owned(),
    };
    plain(found) == plain(expected)
}

/// Whether `a` and `b` name the same path, as Windows compares them: case folded, with any
/// `\\?\` prefix and either separator.
pub fn same_path(a: &Path, b: &Path) -> bool {
    let fold = |path: &Path| {
        let text = path.to_string_lossy();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
        text.replace('/', "\\").to_lowercase()
    };
    fold(a) == fold(b)
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _infallible = write!(text, "{byte:02x}");
        text
    })
}

/// Whether `dir` lies strictly inside `profile` (the owner's `ProfileImagePath`), compared part
/// by part as Windows compares names: case folded, any `\\?\` prefix and either separator, a
/// trailing separator ignored. The walk opens every part of `dir` without following a reparse
/// point, so its text is where it leads.
pub fn inside_profile(dir: &Path, profile: &Path) -> bool {
    let parts = |path: &Path| -> Vec<String> {
        let text = path.to_string_lossy();
        let text = text
            .strip_prefix(r"\\?\")
            .unwrap_or(&text)
            .replace('/', "\\");
        text.split('\\')
            .filter(|part| !part.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let (dir, profile) = (parts(dir), parts(profile));
    let drive = profile
        .first()
        .is_some_and(|first| first.len() == 2 && first.ends_with(':'));
    drive && dir.len() > profile.len() && dir.starts_with(&profile)
}
