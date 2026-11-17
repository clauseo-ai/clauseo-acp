//! The guard's record of the refusals the lawyer is told about. Codex sends no event for a call
//! its hook blocked (codex-acp 2.2.2 ignores `hook/completed`), so with [`FLAG`] and a file
//! (Codex's hook passes `<data>/browser/guard/<session>/guard-blocks.jsonl`, [`file`],
//! `agent/loads/codex.rs`) and [`ID_FLAG`] with its identity, a refusal for a held-open file or
//! the `.codex` folder is appended to that file as one JSON line, a [`Record`], and the host
//! reads it back during the turn to show "Clauseo stopped a step"
//! (`actor/turn/updates/guard_blocks.rs`). Claude Code's entry passes no file: Claude shows the
//! blocked call as a failed step of its own.
//!
//! The guard runs outside the agent's sandbox, so it writes only the file the host made: the host
//! makes a folder of the session's own, create-new, then the file in it empty, with `O_EXCL`
//! ([`make`]), under `<data>/browser`, which the agent's shells and patch tools are denied, and
//! reads the file's identity from the file it made (`write::RecordId`). That identity goes in
//! the trust-hashed hook command beside the path. The guard never creates the file, opens it
//! through no link at its last component, and writes only a regular file with one link and that
//! identity, all read from the file it opened (`write::open_append_record`). So a hard link, a
//! symlink, a file moved in at its place, or a link at one of its folders (to a shell profile,
//! say) is never written through. Each line starts on a line of its own (a newline first), so a
//! line cut short before it never spoils it.
//!
//! Best effort: a file that cannot be opened or written (none, a link, a pipe, no room) is skipped
//! without a word, and the exit code and the reason never change. The line holds no agent text
//! but the tool's name: the path is the guard's own listing of the held file.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use clauseo_protocol::{MatterPath, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Refusal;
use crate::clock::{Clock, SystemClock};
use crate::write::RecordId;

/// The record file's name, in the session's guard folder ([`folder`]).
pub(crate) const FILE_NAME: &str = "guard-blocks.jsonl";

/// The folder under `<data>/browser` that holds each session's guard folder. Nothing else
/// writes there: the browser's own folders under `<data>/browser` are `run` (each session's
/// `--cwd`), its install and its settings files (`browser.rs`, `launch_checks/folders.rs`).
pub(crate) const FOLDER: &str = "guard";

/// The argument that names the record file, followed by the file; after any other argument.
pub(crate) const FLAG: &str = "--record";

/// The argument that gives the record file's identity ([`RecordId::hex`]), followed by it;
/// after [`FLAG`] and its file. With none, or one that does not parse, nothing is recorded.
pub(crate) const ID_FLAG: &str = "--record-id";

/// The guard folder of the session `session` (its run id): `<data>/browser/guard/<session>`,
/// in `<data>/browser`, which no agent's sandboxed shell or patch tool may write (Codex's
/// sandbox names no writable root there, `agent/codex_fence.rs`; Claude's `Read` and `Edit`
/// deny of `browser/**`). A Codex command its reviewer approves runs unsandboxed.
pub(crate) fn folder(data: &Path, session: &str) -> PathBuf {
    data.join(crate::browser::CONFIG_DIR)
        .join(FOLDER)
        .join(session)
}

/// The record file in the guard folder `folder`.
pub(crate) fn file(folder: &Path) -> PathBuf {
    folder.join(FILE_NAME)
}

/// Makes the record file of the session `session` under the data folder `data`, empty: the
/// parent of the guard folders through no link below `data` (one that is a link or no folder
/// is first removed, never followed, [`unlink_parent`]), then the session's guard folder
/// ([`folder`]) with a create-new call, then the file in it with `O_EXCL`. Anything already at
/// the session's folder (a folder, a file, a symlink, a junction or another reparse point) fails
/// with `AlreadyExists`, never followed and left as it is. The file and its identity, read from
/// the file made. A file not made takes the folder made for it back. File system calls: the
/// launch prep's, off the actor (`launch_checks/folders.rs` `make_guard`).
pub(crate) fn make(data: &Path, session: &str) -> std::io::Result<(PathBuf, RecordId)> {
    let folder = folder(data, session);
    let parent = folder.parent().unwrap_or(data);
    unlink_parent(parent);
    crate::write::create_private_dirs_under(data, parent)?;
    crate::write::create_private_dir(&folder)?;
    let file = file(&folder);
    match crate::write::create_record(&file) {
        Ok(id) => Ok((file, id)),
        Err(error) => {
            drop(crate::write::remove_tree(&folder));
            Err(error)
        }
    }
}

/// Removes `parent` (`<data>/browser/guard`) when it is there but is no real folder: a symlink,
/// a junction, another reparse point or a file. The link itself goes ([`remove_link`]), never
/// what it points to, with a warning, so [`make`] makes a real folder in its place and the
/// launch goes on, as the browser install replaces a link at its version folder
/// (`browser/install.rs`). Left as it was, every launch but Claude's would fail. A link it cannot
/// remove is left, and the launch is refused below.
///
/// [`remove_link`]: crate::write::remove_link
fn unlink_parent(parent: &Path) {
    if std::fs::symlink_metadata(parent).is_err() || crate::write::is_real_dir(parent) {
        return;
    }
    match crate::write::remove_link(parent) {
        Ok(()) => tracing::warn!(
            "the guard records' folder was a link or no folder; the link was removed, not followed"
        ),
        Err(error) => tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "the guard records' folder is a link or no folder, and the link was not removed"
        ),
    }
}

/// The longest tool name a record keeps; a longer one is cut.
const MAX_TOOL: usize = 64;

/// Why a refusal was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reason {
    /// The file is open in Word or another program.
    HeldOpen,
    /// The call writes in, or under Codex's hook names, the matter's `.codex` folder.
    CodexFolder,
}

/// Whose hook refused the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Agent {
    /// Claude Code's entry, the one that passes `--matter`.
    Claude,
    /// Codex's hook, with no `--matter`.
    Codex,
}

/// One recorded refusal, one JSON line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    /// When, by the wall clock.
    pub(crate) at: Timestamp,
    /// Whose hook it was.
    pub(crate) agent: Agent,
    /// The hook input's tool name (`apply_patch`, `Bash`, ...), at most [`MAX_TOOL`] bytes.
    pub(crate) tool: String,
    /// Why it was refused.
    pub(crate) reason: Reason,
    /// The held file as the matter lists it; none for the `.codex` folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<MatterPath>,
}

/// The absolute file after [`FLAG`] in `args` and the identity after [`ID_FLAG`], if both
/// are there.
pub(super) fn target(args: &[OsString]) -> Option<(PathBuf, RecordId)> {
    let after = |flag: &str| {
        let at = args.iter().position(|arg| arg == flag)?;
        args.get(at.saturating_add(1))
    };
    let file = after(FLAG)
        .map(PathBuf::from)
        .filter(|file| file.is_absolute())?;
    let id = RecordId::parse(after(ID_FLAG)?.to_str()?)?;
    Some((file, id))
}

/// The record of `refusal` for the hook input `bytes`, from Claude Code's entry when `claude`;
/// `None` for a refusal the lawyer is not told about (`stetdoc mcp`, Codex's home, a command too
/// nested to read).
pub(super) fn record_of(refusal: &Refusal, bytes: &[u8], claude: bool) -> Option<Record> {
    let (reason, path) = match refusal {
        Refusal::HeldOpen(path) => (Reason::HeldOpen, Some(path.clone())),
        Refusal::CodexFolder => (Reason::CodexFolder, None),
        // No notice kind names Codex's home yet: the agent is told, the lawyer reads its reply.
        Refusal::StetdocServe | Refusal::CodexHome | Refusal::Unread => return None,
    };
    let tool = serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|hook| hook.get("tool_name")?.as_str().map(cut))
        .unwrap_or_default();
    Some(Record {
        at: SystemClock.now().wall,
        agent: if claude { Agent::Claude } else { Agent::Codex },
        tool,
        reason,
        path,
    })
}

/// `tool`, cut to at most [`MAX_TOOL`] bytes at a character boundary.
fn cut(tool: &str) -> String {
    let mut end = tool.len().min(MAX_TOOL);
    while !tool.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    tool.get(..end).unwrap_or_default().to_owned()
}

/// Appends `record` to `file` as one line, a newline before and after it, in one write, when
/// `file` is the record the host made, with the identity `id`; any failure is dropped.
pub(super) fn append(file: &Path, id: RecordId, record: &Record) {
    let mut line = vec![b'\n'];
    if serde_json::to_writer(&mut line, record).is_err() {
        return;
    }
    line.push(b'\n');
    if let Ok(mut out) = crate::write::open_append_record(file, id) {
        drop(out.write_all(&line));
    }
}
