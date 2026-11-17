//! The conversation's own sessions, under Codex's hook. Codex runs the hook for each tool call
//! of every thread its app-server runs, with that thread's `session_id`, which Codex sets and the
//! model cannot (a `multi_agent` child carries its parent's). With [`FLAG`] and a file (Codex's
//! hook passes `<data>/browser/guard/<session>/sessions.json`, [`file`]), the guard answers only
//! a call whose `session_id` is listed there: the host writes the ids of the conversation's
//! sessions (`session/new`, `session/load` and `session/resume` give the same id, [`add`]) after
//! the session call returns and before its first prompt, in the guard folder no agent may write.
//! Any other call is refused with [`REFUSED`], and so is every call when the guard can't tell:
//! stdin it can't read or parse, no `session_id`, a file that is missing, unreadable, too big or
//! not version 1 of the ids file ([`IdsFile`]). A refusal is exit 2 with its reason on stderr,
//! which Codex takes as a block; with [`FLAG`] the guard process also answers that way on a panic
//! and past its own deadline ([`arm`]).
//!
//! Then, for a listed session, the guard's other checks apply as before, and one more: a
//! `view_image` of a path in Clauseo's data folder is refused unless the path, links followed,
//! lies in one of the session's own folders that the hook names after [`OWN_FLAG`] (its work
//! folder and its matter's notes); the data fence's own check, `agent/data_fence.rs`.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The argument that names the file of the conversation's session ids, followed by the file;
/// after the record arguments.
pub(crate) const FLAG: &str = "--sessions";

/// The argument that names one of the session's own folders in the data folder, followed by
/// the folder; once per folder, after [`FLAG`] and its file. A `view_image` there is not refused.
pub(crate) const OWN_FLAG: &str = "--own-folder";

/// The ids file's name, in the session's guard folder (`record::folder`).
pub(crate) const FILE_NAME: &str = "sessions.json";

/// What the model is told of a call from a session that is not the conversation's.
pub const REFUSED: &str = "Clauseo runs tools only in the conversation the lawyer started.";

/// What the model is told when the guard did not finish within [`DEADLINE`].
pub const TOO_LATE: &str = "Clauseo could not check this command in time. Try it again.";

/// What the model is told when the guard failed inside (a panic).
pub const FAILED: &str = "Clauseo could not check this command. Try it again.";

/// What the model is told of a `view_image` of a file in Clauseo's data folder.
const DATA_IMAGE: &str = "Clauseo doesn't let the agent open files in Clauseo's own data folder.";

/// How long the guard process may take before it refuses on its own ([`arm`]). A normal call
/// takes milliseconds; a held-open check over a big matter folder on a slow disk takes longer,
/// so this leaves room for it, at most half Codex's own hook timeout (`agent/loads/codex.rs`).
pub const DEADLINE: Duration = Duration::from_secs(15);

/// The ids file's format version: [`IdsFile::version`].
const VERSION: u32 = 1;

/// The ids file: `{"version": 1, "sessions": ["<id>", …]}`, nothing else. Another version, a
/// field more or less, or another shape is no ids file to the guard ([`read`]).
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdsFile {
    /// [`VERSION`].
    version: u32,
    /// The conversation's session ids, none empty.
    sessions: Vec<String>,
}

/// The longest ids file read; a longer one is refused.
const MAX_FILE: u64 = 64 * 1024;

/// Whether the arguments ask for the session check: [`FLAG`] is there (with a file or not).
pub(super) fn wanted(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == FLAG)
}

/// The absolute file after [`FLAG`], if any.
pub(super) fn file_arg(args: &[OsString]) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == FLAG)?;
    args.get(at.saturating_add(1))
        .map(PathBuf::from)
        .filter(|file| file.is_absolute())
}

/// The absolute folders after each [`OWN_FLAG`].
pub(super) fn own_args(args: &[OsString]) -> Vec<PathBuf> {
    args.iter()
        .zip(args.iter().skip(1))
        .filter(|(flag, _)| *flag == OWN_FLAG)
        .map(|(_, folder)| PathBuf::from(folder))
        .filter(|folder| folder.is_absolute())
        .collect()
}

/// The ids file in the guard folder `folder`.
pub fn file(folder: &Path) -> PathBuf {
    folder.join(FILE_NAME)
}

/// The data folder of the ids file `file`: `<data>/browser/guard/<session>/sessions.json`.
fn data_of(file: &Path) -> Option<&Path> {
    file.ancestors().nth(4)
}

/// The reason the call in `bytes` (`None`: stdin could not be read) is refused under the session
/// check with the ids file `file` and the session's own folders `own`, if it is: [`REFUSED`]
/// unless its `session_id` is listed, then the `view_image` rule.
pub(super) fn refusal(
    bytes: Option<&[u8]>,
    file: Option<&Path>,
    own: &[PathBuf],
) -> Option<&'static str> {
    let hook = bytes.and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
    let id = hook
        .as_ref()
        .and_then(|hook| hook.get("session_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let (Some(hook), Some(id), Some(file)) = (hook.as_ref(), id, file) else {
        return Some(REFUSED);
    };
    if !read(file).is_some_and(|ids| ids.iter().any(|known| known == id)) {
        return Some(REFUSED);
    }
    view_image_in_data(hook, file, own).then_some(DATA_IMAGE)
}

/// The ids in `file`: an [`IdsFile`] of [`VERSION`] whose ids are none empty, the file at most
/// [`MAX_FILE`] bytes; `None` for anything else, which the guard refuses as an unknown session.
/// The guard's one reader of the file.
pub fn read(file: &Path) -> Option<Vec<String>> {
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .ok()?
        .take(MAX_FILE.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_FILE) {
        return None;
    }
    let parsed: IdsFile = serde_json::from_slice(&bytes).ok()?;
    let ids = parsed.sessions;
    (parsed.version == VERSION && ids.iter().all(|id| !id.is_empty())).then_some(ids)
}

/// How many times [`add`] tries the rename that puts the ids file in place, and the pause
/// between two tries: about half a second in all, for a scanner (Defender on Windows) that holds
/// the new file for a moment. Harmless elsewhere.
const RENAME_TRIES: u32 = 5;
const RENAME_PAUSE: Duration = Duration::from_millis(125);

/// Adds the session `id` to the ids file in the guard folder `folder`, keeping the ids already
/// there (a fork's new id joins its conversation's), written whole and private
/// (`write::replace_private_file`), the rename tried up to [`RENAME_TRIES`] times
/// [`RENAME_PAUSE`] apart; the last try's error is the result. Nothing is written when `id` is
/// listed already. The host's one writer of the file, off the actor.
pub fn add(folder: &Path, id: &str) -> std::io::Result<()> {
    add_with(folder, id, &crate::write::rename_file, &pause)
}

/// Blocks the calling thread for `time`: the pause between two tries of the ids file's rename.
/// Bound: at most 4 retries after the first try ([`RENAME_TRIES`] 5), each pause
/// [`RENAME_PAUSE`] 125 ms, so at most 500 ms of pauses in total per [`add`].
#[expect(
    clippy::disallowed_methods,
    reason = "a pause between two renames, off the actor, in real time: at most 4 retries, 125 ms \
              apart, 500 ms in total"
)]
fn pause(time: Duration) {
    // At most 4 retries, 125 ms apart, 500 ms in total (RENAME_TRIES, RENAME_PAUSE).
    std::thread::sleep(time);
}

/// [`add`] with `rename` in place of `std::fs::rename` and `pause` in place of the real pause.
fn add_with(
    folder: &Path,
    id: &str,
    rename: &dyn Fn(&Path, &Path) -> std::io::Result<()>,
    pause: &dyn Fn(Duration),
) -> std::io::Result<()> {
    let file = file(folder);
    let mut ids = read(&file).unwrap_or_default();
    if ids.iter().any(|known| known == id) {
        return Ok(());
    }
    if id.is_empty() {
        return Err(std::io::Error::other("an empty session id"));
    }
    ids.push(id.to_owned());
    let text = serde_json::to_vec(&IdsFile {
        version: VERSION,
        sessions: ids,
    })
    .map_err(std::io::Error::other)?;
    crate::write::replace_private_file_by(&file, &text, |temp, path| {
        let mut tries = 1;
        loop {
            match rename(temp, path) {
                Err(error) if tries < RENAME_TRIES => {
                    tracing::debug!(%error, tries, "the ids file's rename failed; trying again");
                    pause(RENAME_PAUSE);
                    tries += 1;
                }
                done => return done,
            }
        }
    })
}

/// Whether the call is a `view_image` whose path, taken from the input's `cwd`, resolves into
/// the data folder of `file` outside the session's own folders `own`, by the data fence's check
/// (links followed, case folded as the file system folds it; an own folder counts only while it
/// resolves to its own place, not through a link). A `view_image` with no path string, or no
/// absolute `cwd` for a relative one, counts as in it; no data folder to compare with counts as
/// in it too.
fn view_image_in_data(hook: &Value, file: &Path, own: &[PathBuf]) -> bool {
    if hook.get("tool_name").and_then(Value::as_str) != Some("view_image") {
        return false;
    }
    let path = hook
        .get("tool_input")
        .and_then(|input| input.get("path"))
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let cwd = hook
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|cwd| cwd.is_absolute());
    let (Some(path), Some(data)) = (path, data_of(file)) else {
        return true;
    };
    let Some(cwd) = cwd.or_else(|| path.is_absolute().then(|| path.clone())) else {
        return true;
    };
    let check = crate::agent::data_fence::FenceCheck {
        data: data.to_path_buf(),
        allowed: own.to_vec(),
        cwd,
        home: None,
        paths: vec![path],
        roots: Vec::new(),
        words: Vec::new(),
        health: crate::agent::data_fence::DataFenceHealth::default(),
        vars: Vec::new(),
    };
    let net = crate::network_paths::NetEnv::current();
    let disk = crate::network_paths::resolve::Disk::real(&net);
    check.touches_data_on(crate::network_paths::FOLD_CASE, &disk)
}

/// With [`FLAG`] in `args` (Codex's hook), makes this process refuse on its own when the guard
/// can't answer: a panic writes [`FAILED`] and exits 2, and past [`DEADLINE`] a watchdog thread
/// writes [`TOO_LATE`] and exits 2. A process-wide change, for the guard's process only
/// (`main` calls it first); Claude Code's entry, without [`FLAG`], is left as it was.
pub fn arm(args: &[OsString]) {
    if wanted(args) {
        arm_with(DEADLINE);
    }
}

/// [`arm`] with the deadline `deadline`.
#[expect(
    clippy::disallowed_methods,
    reason = "the guard process's own watchdog thread waits out its deadline in real time"
)]
pub(super) fn arm_with(deadline: Duration) {
    std::panic::set_hook(Box::new(|_| refuse_now(FAILED)));
    let watchdog = std::thread::Builder::new()
        .name("clauseo-guard-deadline".to_owned())
        .spawn(move || {
            std::thread::sleep(deadline);
            refuse_now(TOO_LATE);
        });
    if watchdog.is_err() {
        refuse_now(TOO_LATE);
    }
}

/// Writes `line` and a newline to stderr and exits 2 ([`super::DENY`]) at once.
#[expect(
    clippy::exit,
    reason = "the guard process refuses from a panic hook or its watchdog thread, where nothing \
              returns to `main`"
)]
fn refuse_now(line: &str) -> ! {
    let mut errors = std::io::stderr();
    // Nothing more can be done when stderr is gone; the exit code is still 2.
    drop(
        errors
            .write_all(line.as_bytes())
            .and_then(|()| errors.write_all(b"\n"))
            .and_then(|()| errors.flush()),
    );
    std::process::exit(super::DENY)
}

/// One decision of the guard under the session check, one JSON line in the session's record file
/// (`record.rs`, beside its refusal records, which it does not parse as), for the host to log
/// (`actor/turn/updates/guard_blocks.rs`): the session, the tool, the decision and how long the
/// guard took. No input content but the tool's name and the session id, each cut to 64 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Decision {
    /// Always `"decision"`: what the line is.
    pub(crate) guard: String,
    pub(crate) session: String,
    pub(crate) tool: String,
    /// `"allowed"` or `"refused"`.
    pub(crate) verdict: String,
    /// Milliseconds from the guard's start to its decision.
    pub(crate) ms: u64,
}

/// A monotonic instant, for [`record_decision`]'s elapsed time.
#[expect(
    clippy::disallowed_methods,
    reason = "the guard process times its own decision"
)]
pub(super) fn now() -> std::time::Instant {
    std::time::Instant::now()
}

/// Appends the [`Decision`] on the call in `bytes` (refused when `refused`), since `started`, to
/// the record file the arguments name, as a refusal record is appended (`record::append`: only
/// the file the host made, never through a link); nothing without one. Best effort.
pub(super) fn record_decision(
    args: &[OsString],
    bytes: &[u8],
    refused: bool,
    started: std::time::Instant,
) {
    let Some((file, id)) = super::record::target(args) else {
        return;
    };
    let hook = serde_json::from_slice::<Value>(bytes).ok();
    let field = |key: &str| {
        let text = hook
            .as_ref()
            .and_then(|hook| hook.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default();
        super::record::cut(text)
    };
    let decision = Decision {
        guard: "decision".to_owned(),
        session: field("session_id"),
        tool: field("tool_name"),
        verdict: if refused { "refused" } else { "allowed" }.to_owned(),
        ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
    let mut line = vec![b'\n'];
    if serde_json::to_writer(&mut line, &decision).is_err() {
        return;
    }
    line.push(b'\n');
    if let Ok(mut out) = crate::write::open_append_record(&file, id) {
        drop(out.write_all(&line));
    }
}

/// Logs the decision lines among `lines` of a session's record file (the host's, off the actor).
pub(crate) fn log_decisions<'a>(lines: impl Iterator<Item = &'a [u8]>) {
    for decision in lines.filter_map(|line| serde_json::from_slice::<Decision>(line).ok()) {
        tracing::info!(
            session = %decision.session,
            tool = %decision.tool,
            verdict = %decision.verdict,
            took_ms = decision.ms,
            "the Codex guard decided"
        );
    }
}

#[cfg(test)]
#[path = "sessions/add_tests.rs"]
mod add_tests;
#[cfg(test)]
#[path = "sessions/tests.rs"]
mod tests;
