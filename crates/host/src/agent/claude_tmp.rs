//! Claude's short temp folder, `/tmp/clsb-<8 random>` ([`claude_tmp_dir`]): Claude Code's
//! `CLAUDE_CODE_TMPDIR` for a Claude session whose sandbox runs (`agent::loads::sandbox`).
//! Claude Code 2.1.293 makes its sandboxed commands' `TMPDIR` under `<it>/claude-<uid>` only when
//! that path is at most 44 bytes; otherwise it falls back to `/tmp/claude-<uid>`, which every
//! session shares and every sandboxed command may write. The run folder's `tmp` is far longer,
//! so this folder is made beside it ([`MAX`], [`fits`]): 0700, its name random, made on the
//! launch-prep thread ([`make_claude_tmp`]), named in the run folder's `claude-tmp` file, and
//! removed with the run folder or, after a crash, with the earlier run's folders at the next
//! start ([`remove_claude_tmp`]). The agent's own `TMPDIR` stays the run folder's `tmp`
//! (`agent::temp`); only sandboxed commands use this one.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::log_file::Cause;

/// The variable Claude Code takes its temp folder from.
const CLAUDE_TMP_VAR: &str = "CLAUDE_CODE_TMPDIR";

/// The folder the short temp folders are made in.
const ROOT: &str = "/tmp";

/// The short temp folder's name before its random part.
const PREFIX: &str = "clsb-";

/// The random part's length and alphabet.
const RANDOM_LEN: usize = 8;
const ALPHABET: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// The longest `<CLAUDE_CODE_TMPDIR>/claude-<uid>` Claude Code 2.1.293 uses.
pub(crate) const MAX: usize = 44;

/// `<dir>/claude-<uid>` for the longest uid (10 digits), as given and as macOS resolves `/tmp`
/// (`/private/tmp`).
const LONGEST_UID: &str = "claude-4294967295";

/// The file in the run folder that names its short temp folder.
const MARKER: &str = "claude-tmp";

/// A new short temp folder's path, its name random; none when no random bytes could be had.
/// Paths only: nothing is made.
pub(crate) fn claude_tmp_dir() -> Option<PathBuf> {
    let mut bytes = [0u8; RANDOM_LEN];
    getrandom::fill(&mut bytes).ok()?;
    let random: String = bytes
        .iter()
        .map(|byte| {
            let at = usize::from(*byte) % ALPHABET.len();
            ALPHABET.get(at).map_or('a', |letter| char::from(*letter))
        })
        .collect();
    Some(Path::new(ROOT).join(format!("{PREFIX}{random}")))
}

/// Whether `<dir>/claude-<uid>` fits [`MAX`] for any uid, as given and under `/private`.
pub(crate) fn fits(dir: &Path) -> bool {
    let longest = |dir: &Path| dir.join(LONGEST_UID).as_os_str().len();
    let private = Path::new("/private").join(dir.strip_prefix("/").unwrap_or(dir));
    longest(dir) <= MAX && longest(&private) <= MAX
}

/// Whether `dir` is a short temp folder's path as [`claude_tmp_dir`] makes them.
fn is_short_temp(dir: &Path) -> bool {
    let Some(name) = dir
        .strip_prefix(ROOT)
        .ok()
        .and_then(Path::to_str)
        .and_then(|rest| rest.strip_prefix(PREFIX))
    else {
        return false;
    };
    name.len() == RANDOM_LEN && name.bytes().all(|byte| ALPHABET.contains(&byte))
}

/// Makes `dir` (0700, nothing already there) and names it in `run_root`'s `claude-tmp` file,
/// on the launch-prep thread; whether it was made. When it was not, a warning says why and the
/// session gets no sandbox, so its commands ask.
pub(crate) fn make_claude_tmp(run_root: &Path, dir: &Path) -> bool {
    if !is_short_temp(dir) || !fits(dir) {
        tracing::warn!("Claude's temp folder has an unexpected name; its sandbox is off");
        return false;
    }
    let marker = run_root.join(MARKER);
    let text = dir.as_os_str().as_encoded_bytes();
    let made = crate::write::replace_private_file(&marker, text)
        .and_then(|()| crate::write::create_private_dir(dir));
    if let Err(error) = made {
        tracing::warn!(error = %Cause(&error), "Claude's temp folder was not made; its sandbox is off");
        if crate::write::remove_old_file(&marker).is_err() {
            tracing::warn!("the note of Claude's temp folder stays");
        }
        return false;
    }
    true
}

/// Removes the short temp folder `run_root`'s `claude-tmp` file names, when it is one of ours:
/// named as [`claude_tmp_dir`] makes them, a real folder, owned by this user. Off the actor.
pub(crate) fn remove_claude_tmp(run_root: &Path) {
    let Ok(text) = std::fs::read(run_root.join(MARKER)) else {
        return;
    };
    let Ok(text) = String::from_utf8(text) else {
        return;
    };
    let dir = PathBuf::from(text);
    if !is_short_temp(&dir) || !crate::write::is_real_dir(&dir) || !owned_by_me(&dir) {
        return;
    }
    if let Err(error) = crate::write::remove_tree(&dir) {
        tracing::warn!(error = %Cause(&error), "Claude's temp folder stays");
    }
}

/// Whether `dir` belongs to this process's user.
#[cfg(unix)]
fn owned_by_me(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(dir).is_ok_and(|meta| meta.uid() == nix::unistd::getuid().as_raw())
}

/// No short temp folder is made on Windows.
#[cfg(not(unix))]
fn owned_by_me(_dir: &Path) -> bool {
    false
}

/// `dir` as `CLAUDE_CODE_TMPDIR`.
pub(crate) fn claude_env(dir: &Path) -> BTreeMap<OsString, OsString> {
    BTreeMap::from([(OsString::from(CLAUDE_TMP_VAR), dir.as_os_str().to_owned())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_short_temp_folder_fits_claude_codes_limit_for_any_uid() {
        for _ in 0..32 {
            let dir = claude_tmp_dir().unwrap();
            assert!(is_short_temp(&dir), "{}", dir.display());
            assert!(fits(&dir), "{}", dir.display());
            let private = Path::new("/private").join(dir.strip_prefix("/").unwrap());
            assert!(private.join(LONGEST_UID).as_os_str().len() <= MAX);
        }
        assert_ne!(claude_tmp_dir(), claude_tmp_dir());
        // The run folder's own temp folder never fits: why this one exists.
        let run = Path::new("/Users/l/Library/Application Support/Clauseo/run/t-1/tmp");
        assert!(!fits(run));
        assert!(!is_short_temp(Path::new("/tmp/clsb-../../x")));
        assert!(!is_short_temp(Path::new("/var/clsb-abcdefgh")));
    }

    #[cfg(unix)]
    #[test]
    fn it_is_made_private_and_removed_with_its_marker() {
        use std::os::unix::fs::PermissionsExt;
        let run = std::env::temp_dir().join(format!("clauseo-claude-tmp-{}", std::process::id()));
        crate::write::remove_tree(&run).unwrap();
        crate::write::create_dir_all(&run).unwrap();
        let dir = claude_tmp_dir().unwrap();
        assert!(make_claude_tmp(&run, &dir));
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert!(!make_claude_tmp(&run, &dir), "nothing already there");
        // The failed second make took its marker back: write it again for the removal.
        crate::write::write_file(&run.join(MARKER), dir.as_os_str().as_encoded_bytes()).unwrap();
        remove_claude_tmp(&run);
        assert!(!dir.exists());
        crate::write::write_file(&run.join(MARKER), b"/etc").unwrap();
        remove_claude_tmp(&run);
        assert!(Path::new("/etc").exists());
        crate::write::remove_tree(&run).unwrap();
    }
}
