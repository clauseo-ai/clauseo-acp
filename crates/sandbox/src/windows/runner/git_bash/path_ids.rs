//! Which of the lawyer's `PATH` entries the agent's `PATH` drops ([`dropped_entries`]), judged
//! by what each entry is on disk, never by its text: so a short name (`C:\PROGRA~1\Git\cmd`), a
//! quoted entry, `..`, a trailing dot or a `\\?\` form can't keep the lawyer's `git.exe` there.
//!
//! An entry goes when:
//! - it isn't a full local path once its quotes and spaces are trimmed: relative, drive-relative,
//!   a device form (`\\.\`, `\\?\` other than `\\?\X:\`) or empty;
//! - it is a network path: UNC (`\\server\share`, `\\?\UNC\`) or a network drive. Neither is
//!   ever opened: no name is looked up, no handle made;
//! - it, or a folder above it, is a reparse point (a junction or a link), opened as itself;
//! - it, or a folder above it, is one of the lawyer's Git installs: the same file ID on the
//!   same volume as the install folder, opened through its links.
//!
//! The entry is normalized as Windows' own search normalizes it first (`GetFullPathNameW`):
//! `.` parts go, `..` takes the part before it, each part loses its trailing dots and spaces.
//! An entry that isn't there, or can't be opened, stays: it can't hold anything the agent runs.

/// A file's identity: its volume's serial number and its 128-bit file ID.
pub(crate) type FileId = (u64, u128);

/// The machine's answers, which `os.rs` gives as the desktop user and the tests fake.
pub(crate) trait FileIds {
    /// Whether the drive `drive` (`C:`) is a network drive; asking never touches the network.
    fn remote(&mut self, drive: &str) -> bool;
    /// The folder `path` (`C:\a\b`) opened as itself, never through a link at its name: its
    /// identity and whether it is a reparse point. `None` when it can't be opened.
    fn plain_id(&mut self, path: &str) -> Option<(FileId, bool)>;
    /// The folder `path` opened through its links: its identity. `None` when it can't be.
    fn root_id(&mut self, path: &str) -> Option<FileId>;
}

/// The entries of `path` (a `;` list, as written) the agent's `PATH` drops, given the lawyer's
/// Git installs `roots` (full local paths; a network one is never opened).
pub(crate) fn dropped_entries(path: &str, roots: &[String], ids: &mut impl FileIds) -> Vec<String> {
    let mut found: Vec<FileId> = Vec::new();
    for (drive, parts) in roots.iter().filter_map(|root| local(root)) {
        if !ids.remote(&drive) {
            found.extend(ids.root_id(&joined(&drive, &parts)));
        }
    }
    let roots = found;
    path.split(';')
        .filter(|entry| drops(entry, &roots, ids))
        .map(str::to_owned)
        .collect()
}

/// Whether `entry` goes ([`dropped_entries`]).
fn drops(entry: &str, roots: &[FileId], ids: &mut impl FileIds) -> bool {
    let Some((drive, parts)) = local(entry) else {
        return true;
    };
    if ids.remote(&drive) {
        return true;
    }
    if ids.plain_id(&joined(&drive, &parts)).is_none() {
        return false;
    }
    (0..=parts.len()).any(|depth| {
        let prefix = parts.get(..depth).unwrap_or_default();
        match ids.plain_id(&joined(&drive, prefix)) {
            Some((id, reparse)) => reparse || roots.contains(&id),
            None => false,
        }
    })
}

/// `text` as a full local path, normalized: its drive (`C:`) and parts. `None` for anything
/// else ([`dropped_entries`]).
fn local(text: &str) -> Option<(String, Vec<String>)> {
    let text = text.trim().trim_matches('"').trim().replace('/', "\\");
    let text = match text.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_owned(),
        None if text.starts_with(r"\\") => return None,
        None => text,
    };
    let mut chars = text.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let rest = chars.as_str().strip_prefix(':')?;
    if !rest.starts_with('\\') || text.contains('\0') {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for part in rest.split('\\') {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            _ => {
                let part = part.trim_end_matches(['.', ' ']);
                if !part.is_empty() {
                    parts.push(part.to_owned());
                }
            }
        }
    }
    Some((format!("{}:", letter.to_ascii_uppercase()), parts))
}

/// `C:` and `parts` as `C:\a\b`; the drive's root alone as `C:\`.
fn joined(drive: &str, parts: &[String]) -> String {
    format!(r"{drive}\{}", parts.join("\\"))
}
