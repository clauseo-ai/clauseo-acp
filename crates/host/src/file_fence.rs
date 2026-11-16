//! The file-tool fence: before each of Claude Code's file tools runs (`Read`, `Edit`, `Write`,
//! `MultiEdit`, `NotebookEdit`, `Grep`, `Glob`), on every OS, the pre-write guard's hook
//! resolves the path the tool names to where it really lands and refuses the tool when that is
//! inside a fenced place: Clauseo's data folder (what `data_fence::LaunchDeny` denies there),
//! another matter's folder the conversation is not invited into, or a protected folder.
//!
//! Why: Claude's `Read` and `Edit` deny rules match the path's text. On Windows an 8.3 short
//! name in one segment (`C:\Users\CLAUSE~2\…`) got past them and another matter's file reached
//! the model; no list of spellings covers every long and short mix, case, link and prefix. The
//! fence compares where the path lands ([`real`]: `GetFinalPathNameByHandleW` on Windows,
//! `realpath` on macOS and Linux, never touching the network), and also by file identity
//! (volume and file number), so a bind mount, a firmlinked `/System/Volumes/Data` path or a
//! second mount point of a volume is caught too.
//!
//! It only ever refuses: a tool it lets go is left to every other check (the deny rules, the
//! permission mode, the host's auto-deny, the held-open guard).
//!
//! - The fence file ([`FenceFile`]): the host writes it in the session's run folder at each
//!   Claude launch ([`FILE_NAME`], which the data folder's rules deny to the agent's tools,
//!   `data_fence::DENIED`), and passes its path and its SHA-256 to the hook ([`FLAG`]): a file
//!   that is missing, changed (by a shell command, say) or unreadable refuses every file tool.
//!   It lists each fenced root ([`FencedRoot`]): its spellings, resolved by the host the same
//!   way and as configured; its identity and the identities of the folders above it; the
//!   patterns under it denied to every file tool, and those denied to the writing ones.
//! - Literal paths (`Read`, the writing tools): refused when the path, or a folder above it,
//!   is one a root's patterns cover. A path that does not exist yet resolves through its
//!   deepest existing folder.
//! - Search roots (`Grep`'s `path`, `Glob`'s `path` and the start of its pattern before the
//!   first glob character): refused when the folder is inside a fenced place, and also when it
//!   holds one (a search from the home folder would walk the data folder and other matters).
//!   Refusal, not filtering: Claude Code applies its own deny rules to the results by text, so
//!   a root spelled another way (a short name) would get unfiltered results. The refusal tells
//!   the agent to search a folder inside the matter. A `Glob` pattern that may reach past its
//!   start (`..`, a brace group with a separator, another drive) is refused. The folder is also
//!   walked, following nothing, and each link, junction or mount point below it resolved
//!   ([`links`]): one into a fenced place refuses the search.
//! - Fail closed ([`guard`]): on the fence's path, any error (input it cannot read, a fence
//!   file it cannot trust, a path it cannot vouch for) or a panic refuses, and so does the
//!   fence taking longer than [`guard::DEADLINE`]: Claude Code treats a hook that times out or
//!   exits with another code as no answer and runs the tool. What only
//!   the deny rules still stop: a hook process that cannot start, one Claude Code's own timeout
//!   kills before the fence's deadline (a start that takes most of the 10 seconds), and a
//!   session launched with no guard program.
//! - Network paths ([`real::locate`], told from text and the system's lists, never touched):
//!   inside a matter kept on a share that the session may use (its own, or one it is invited
//!   into) they are judged by their text; outside every such matter they are refused while
//!   [`decide::REFUSE_NETWORK_OUTSIDE_MATTERS`] is on, the one switch.
//! - The refusal's words ([`decide::Denial::reason`]) hold `has been denied` and `your
//!   permission settings`, which the host's refusal detector reads as a refused call.
//!
//! Not caught here: a hard link to a fenced file made elsewhere; a shell command (its own rules,
//! `data_fence::shell_deny`, and the sandboxes); a matter, run folder or notes folder made after
//! the launch (in no fence file until the next launch).

pub(crate) mod decide;
pub(crate) mod guard;
mod links;
mod pattern;
pub(crate) mod real;
#[cfg(test)]
mod tests;
#[cfg(all(test, unix))]
mod tests_decide;
#[cfg(all(test, unix))]
mod tests_share;
#[cfg(all(test, windows))]
mod tests_windows;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agent::data_fence::{LaunchDeny, READ_ONLY};
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::{Disk, Fs, RealFs, Walked, walk};

/// The guard's argument followed by the fence file's path and its SHA-256 in hex.
pub(crate) const FLAG: &str = "--fence";

/// The fence file's name in the session's run folder.
pub(crate) const FILE_NAME: &str = "file-fence.json";

/// The fence file's format version.
const VERSION: u32 = 1;

/// The largest fence file read; a longer one refuses every file tool.
pub(crate) const MAX_FILE: u64 = 8 * 1024 * 1024;

/// What a fenced root is, for the refusal's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RootKind {
    /// Clauseo's data folder.
    Data,
    /// Another matter's folder the conversation is not invited into.
    Matter,
    /// A folder no agent may use (sign-in stores, start-up files).
    Protected,
}

/// A file's identity: its volume or device and its file number (decimal: it may pass 2^64).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct FileId {
    pub(crate) volume: u64,
    pub(crate) file: String,
}

impl FileId {
    /// `id` in the file's form.
    pub(crate) fn of((volume, file): (u64, u128)) -> Self {
        Self {
            volume,
            file: file.to_string(),
        }
    }
}

/// One fenced root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FencedRoot {
    pub(crate) kind: RootKind,
    /// Its paths: each resolved by the network-safe walk, then as given; UTF-8 only.
    pub(crate) spellings: Vec<String>,
    /// Its own identity, when it is local and exists.
    pub(crate) id: Option<FileId>,
    /// The identities of it and of each folder above it that could be read.
    pub(crate) line: Vec<FileId>,
    /// Patterns under it denied to every file tool (`**`: all of it).
    pub(crate) read: Vec<String>,
    /// Patterns under it denied to the tools that write.
    pub(crate) edit: Vec<String>,
}

impl FencedRoot {
    /// The root at `paths` (spellings of one folder), resolved now on the real disk: each local
    /// spelling walked ([`walk`] with [`Disk::real`], the same resolution the guard gives an
    /// agent's path), then each as given; a network one is never touched. `None` when none of
    /// them is UTF-8 (no agent path could be compared with it as text, and none is written).
    pub(crate) fn at(
        kind: RootKind,
        paths: &[&Path],
        (read, edit): (Vec<String>, Vec<String>),
        net: &NetEnv,
    ) -> Option<Self> {
        let disk = Disk::real(net);
        let mut resolved = Vec::new();
        let mut spellings = Vec::new();
        for path in paths {
            if !net.is_network(path)
                && let Walked::Local(real) = walk(path, &disk)
            {
                if let Some(text) = real.to_str() {
                    push_new(&mut spellings, text.to_owned());
                }
                resolved.push(real);
            }
        }
        for path in paths.iter().filter_map(|path| path.to_str()) {
            push_new(&mut spellings, path.to_owned());
        }
        if spellings.is_empty() {
            return None;
        }
        let real = resolved.first();
        let id = real.and_then(|real| RealFs.identity(real).ok().map(FileId::of));
        let line = real.map_or_else(Vec::new, |real| {
            real.ancestors()
                .filter_map(|folder| RealFs.identity(folder).ok().map(FileId::of))
                .collect()
        });
        Some(Self {
            kind,
            spellings,
            id,
            line,
            read,
            edit,
        })
    }
}

/// `item` added to `list` unless it is there.
fn push_new(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// The fence file's contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FenceFile {
    pub(crate) version: u32,
    pub(crate) roots: Vec<FencedRoot>,
}

/// A fence file written: where, and its SHA-256 in hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Written {
    pub(crate) path: PathBuf,
    pub(crate) digest: String,
}

impl FenceFile {
    /// The fence of a Claude launch with the data folder's rules `deny`: the data folder (its
    /// spellings in `deny.spellings`) with `deny.targets` denied to every file tool and each of
    /// [`READ_ONLY`] and all under it to the writing ones. Resolves the data folder on the disk.
    pub(crate) fn launch(deny: &LaunchDeny) -> Self {
        let net = NetEnv::current();
        let paths: Vec<&Path> = deny.spellings.iter().map(PathBuf::as_path).collect();
        let edit = READ_ONLY
            .iter()
            .map(|place| format!("{place}/**"))
            .collect();
        let data = FencedRoot::at(RootKind::Data, &paths, (deny.targets.clone(), edit), &net);
        Self {
            version: VERSION,
            roots: data.into_iter().collect(),
        }
    }

    /// With `roots` added, each denied whole to every file tool: other matters' folders the
    /// conversation is not invited into (`RootKind::Matter`), protected folders
    /// (`RootKind::Protected`). Each is resolved on the disk now.
    pub(crate) fn with_roots<P: AsRef<Path>>(
        self,
        kind: RootKind,
        roots: impl IntoIterator<Item = P>,
    ) -> Self {
        self.with(kind, roots, false)
    }

    /// With `roots` added, each denied whole to the tools that write only (a matter invited to
    /// read).
    pub(crate) fn with_edit_roots<P: AsRef<Path>>(
        self,
        kind: RootKind,
        roots: impl IntoIterator<Item = P>,
    ) -> Self {
        self.with(kind, roots, true)
    }

    fn with<P: AsRef<Path>>(
        mut self,
        kind: RootKind,
        roots: impl IntoIterator<Item = P>,
        edit_only: bool,
    ) -> Self {
        let mut roots = roots.into_iter().peekable();
        if roots.peek().is_none() {
            return self;
        }
        let net = NetEnv::current();
        let all = || vec!["**".to_owned()];
        let patterns = if edit_only {
            (Vec::new(), all())
        } else {
            (all(), Vec::new())
        };
        for root in roots {
            let path = root.as_ref();
            self.roots
                .extend(FencedRoot::at(kind, &[path], patterns.clone(), &net));
        }
        self
    }

    /// The file's bytes: compact JSON.
    pub(crate) fn bytes(&self) -> Vec<u8> {
        // A struct of strings and numbers always serializes.
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Writes the file as [`FILE_NAME`] in `run_root`, replacing any there (a private file,
    /// atomically), and gives its path and digest for the hook's arguments.
    pub(crate) fn write_in(&self, run_root: &Path) -> std::io::Result<Written> {
        let bytes = self.bytes();
        let path = run_root.join(FILE_NAME);
        crate::write::replace_private_file(&path, &bytes)?;
        Ok(Written {
            path,
            digest: digest(&bytes),
        })
    }
}

/// The SHA-256 of `bytes` in lower-case hex.
pub(crate) fn digest(bytes: &[u8]) -> String {
    let digit = |value: u8| char::from_digit(u32::from(value), 16).unwrap_or('0');
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| [digit(byte >> 4), digit(byte & 0xf)])
        .collect()
}

/// Why a fence file can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Untrusted {
    /// It could not be opened or read, or is larger than [`MAX_FILE`].
    Unreadable,
    /// Its digest is not the one the host passed.
    Changed,
    /// It is not a fence file of this version.
    Malformed,
}

/// The fence file at `path`, when its SHA-256 is `want` (hex, any case): read once, never
/// through a link at its name.
pub(crate) fn load(path: &Path, want: &str) -> Result<FenceFile, Untrusted> {
    use std::io::Read as _;
    let file =
        crate::write::open_read_nofollow(path).map_err(|_unreadable| Untrusted::Unreadable)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_unreadable| Untrusted::Unreadable)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE {
        return Err(Untrusted::Unreadable);
    }
    if !digest(&bytes).eq_ignore_ascii_case(want) {
        return Err(Untrusted::Changed);
    }
    let fence: FenceFile =
        serde_json::from_slice(&bytes).map_err(|_malformed| Untrusted::Malformed)?;
    if fence.version != VERSION {
        return Err(Untrusted::Malformed);
    }
    Ok(fence)
}
