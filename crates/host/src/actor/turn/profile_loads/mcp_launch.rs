//! The lawyer's local MCP servers start from a private file (`clauseo-mcp launch <file>`), so
//! their arguments and variables, keychain values included, sit neither in the session request
//! nor on the agent's command line. Claude Agent SDK 0.3.293 starts `claude` with every server
//! entry as `--mcp-config <json>` on argv (`sdk.mjs`:
//! `if(x&&Object.keys(x).length>0)K.push("--mcp-config",ie({mcpServers:x}))`), where any process
//! of the machine can read it; Codex gets the same entries in its session config.
//!
//! - [`entries`] (the session's servers, on the actor): each of the lawyer's stdio servers
//!   becomes `clauseo-mcp launch <run>/mcp/<n>-<name>.json` with no variable but, on Windows,
//!   the ones a Windows program needs from the host (`case_law::windows_env`, as the case-law
//!   entry has). Only the lawyer's servers (Settings › Tools, imported ones included) are
//!   rewritten; Clauseo's own entries keep their shape, and remote servers go as they are until
//!   the stdio-to-HTTP bridge lands (their URL and headers stay on Claude's argv till then).
//! - [`staged`] and [`Staged::written`] (each session call): the files of the servers the call
//!   sends, written off the actor (`write::replace_private_file`: 0600 on Unix, atomic, a link
//!   at the path replaced, never followed) just before the call, from the run's snapshot
//!   (`RunFolders::lawyer`, made once at launch). A file that cannot be written fails the call.
//!   The files stay for the run, so a server the agent restarts mid-session reads its file
//!   again, and go with the run folder.
//! - [`Staged::sent`]: the leftovers sweep spares the session's MCP servers by program
//!   (`agent::leftovers`), so it keeps the real programs beside `clauseo-mcp`: on Unix the mode
//!   `exec`s the real program; on Windows `clauseo-mcp` stays as the server's parent.
//!
//! The file's shape is `clauseo-mcp`'s (`crates/mcp/src/launch.rs`). Nothing here logs a value.
//! With no `clauseo-mcp` (`ProfileHook::mcp_launcher` gives none: no desktop profile), the
//! servers go as before, with a warning.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_acp::McpServer;
use clauseo_protocol::{ErrorCode, MatterId, ThreadId};
use serde::Serialize;
use zeroize::Zeroizing;

use super::super::super::Actor;
use crate::agent::codex::prepare::{Job, off_actor};
use crate::agent::loads::case_law;
use crate::clock::Clock;
use crate::error::Error;

/// The run folder's folder of launch files: `<data>/run/<session>/mcp/`.
const FOLDER: &str = "mcp";

/// The mode of `clauseo-mcp` that starts a server from its file.
const MODE: &str = "launch";
/// The file's version, as `clauseo-mcp launch` reads it.
const VERSION: u32 = 1;
/// The longest server name that goes into a file name (`clauseo_settings`' `MCP_NAME_MAX`).
const NAME_MAX: usize = 64;

/// The file of one server: where it goes and what it holds.
struct File {
    /// `<run>/mcp/<n>-<name>.json`.
    path: PathBuf,
    /// The path as the entry's argument names it.
    arg: String,
    /// The server's program, for the sweep.
    program: PathBuf,
    /// The JSON text, the server's secrets among it.
    bytes: Zeroizing<Vec<u8>>,
}

/// The file's JSON, borrowed from the entry so no unzeroed copy of a value is made.
#[derive(Serialize)]
struct Body<'a> {
    v: u32,
    command: &'a str,
    args: &'a [String],
    env: &'a [(String, String)],
}

/// `servers` (the lawyer's, as the run's snapshot holds them) with each stdio server rewritten
/// to start through `launcher` from its file under `run_root`, and those files. A server whose
/// program or file path is not UTF-8 (no entry could name it) is left out, with a warning.
fn rewrite(
    servers: Vec<McpServer>,
    launcher: &Path,
    run_root: &Path,
    windows_env: &[(String, String)],
) -> (Vec<McpServer>, Vec<File>) {
    let mut entries = Vec::with_capacity(servers.len());
    let mut files = Vec::new();
    for (index, server) in servers.into_iter().enumerate() {
        let McpServer::Stdio {
            name,
            command,
            args,
            env,
        } = server
        else {
            entries.push(server);
            continue;
        };
        let path = run_root.join(FOLDER).join(file_name(index, &name));
        let (Some(arg), Some(program)) = (path.to_str(), command.to_str()) else {
            tracing::warn!(
                server = name.as_str(),
                "an MCP server's program or launch file is not a UTF-8 path; the server is left \
                 out of this session"
            );
            continue;
        };
        let body = Body {
            v: VERSION,
            command: program,
            args: &args,
            env: &env,
        };
        let Ok(bytes) = serde_json::to_vec(&body).map(Zeroizing::new) else {
            tracing::warn!(
                server = name.as_str(),
                "an MCP server's launch file was not made"
            );
            continue;
        };
        let arg = arg.to_owned();
        entries.push(McpServer::Stdio {
            name,
            command: launcher.to_path_buf(),
            args: vec![MODE.to_owned(), arg.clone()],
            env: windows_env.to_vec(),
        });
        files.push(File {
            path,
            arg,
            program: command,
            bytes,
        });
        // `env` and `args` drop here; their values were in the entry before, as in `Sent`.
    }
    (entries, files)
}

/// `<n>-<name>.json`, with the server's place in the run's list first, so two names that differ
/// only in case never share a file where the file system folds case; `<n>.json` for a name
/// that is not name-shaped (the settings refuse one; this only keeps a path out of the name).
fn file_name(index: usize, name: &str) -> String {
    let shaped = !name.is_empty()
        && name.len() <= NAME_MAX
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if shaped {
        format!("{index}-{name}.json")
    } else {
        format!("{index}.json")
    }
}

/// The run's folders `thread`'s session calls use, if it has them.
fn run_of(actor: &Actor, thread: ThreadId) -> Option<&super::super::launch_checks::RunFolders> {
    actor.threads.get(&thread)?.turn.run.as_ref()
}

/// The variables a Windows program needs from the host, as the lawyer's entries get them
/// (`profile_loads::lawyer_servers`); none off Windows.
fn windows_env(actor: &Actor) -> Vec<(String, String)> {
    let host: Vec<(std::ffi::OsString, std::ffi::OsString)> = match &actor.wiring.inherited_env {
        Some(env) => env.clone(),
        None => std::env::vars_os().collect(),
    };
    let lookup = |name: &str| {
        host.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    };
    case_law::windows_env(cfg!(windows), lookup, case_law::windows_directory)
}

/// The lawyer's servers of `run`'s session, `lawyer` (the run's snapshot, or none), as the
/// session gets them: each stdio server through `clauseo-mcp launch`. Pure: the files are
/// written by the session call ([`Staged::written`]).
pub(super) fn entries(
    actor: &Actor,
    run: &super::super::launch_checks::RunFolders,
    lawyer: Vec<McpServer>,
) -> Vec<McpServer> {
    let local = lawyer
        .iter()
        .any(|server| matches!(server, McpServer::Stdio { .. }));
    if !local {
        return lawyer;
    }
    let Some(launcher) = actor.profile.mcp_launcher() else {
        tracing::warn!(
            "no clauseo-mcp to start the lawyer's MCP servers from a file: their arguments and \
             variables go to the agent in its session request"
        );
        return lawyer;
    };
    rewrite(lawyer, &launcher, &run.root, &windows_env(actor)).0
}

/// What `thread`'s next session call writes and the sweep spares; empty without a run, a local
/// server or `clauseo-mcp`.
pub(in super::super) fn staged(actor: &Actor, thread: ThreadId) -> Staged {
    let (Some(run), Some(launcher)) = (run_of(actor, thread), actor.profile.mcp_launcher()) else {
        return Staged::default();
    };
    let lawyer = run.lawyer.servers.clone();
    let (_, files) = rewrite(lawyer, &launcher, &run.root, &[]);
    Staged {
        root: run.root.clone(),
        files,
    }
}

/// The launch files of one session call, before it goes out.
#[derive(Default)]
pub(in super::super) struct Staged {
    /// The run folder.
    root: PathBuf,
    /// The files, each server's.
    files: Vec<File>,
}

impl Staged {
    /// Keeps the files `servers` (what the call sends) start from, and gives `commands` (the
    /// sent servers' programs) with those servers' real programs added, for the sweep.
    pub(in super::super) fn sent(
        &mut self,
        servers: &[McpServer],
        mut commands: Vec<PathBuf>,
    ) -> Vec<PathBuf> {
        let named = |file: &File| {
            servers.iter().any(|server| match server {
                McpServer::Stdio { args, .. } => {
                    matches!(args.as_slice(), [mode, arg] if mode == MODE && *arg == file.arg)
                }
                McpServer::Http { .. } => false,
            })
        };
        self.files.retain(named);
        for file in &self.files {
            if !commands.contains(&file.program) {
                commands.push(file.program.clone());
            }
        }
        commands
    }

    /// `check`, then the files written off the actor; a file that cannot be written fails the
    /// call. Nothing to write answers at once.
    pub(in super::super) async fn written(
        self,
        check: impl Future<Output = Result<(), Error>> + Send + 'static,
        clock: Arc<dyn Clock>,
        matter: Option<MatterId>,
    ) -> Result<(), Error> {
        check.await?;
        let Staged { root, files } = self;
        if files.is_empty() {
            return Ok(());
        }
        let job = Job::short("the MCP servers' launch files", matter);
        off_actor(clock, job, move || write(&root, &files)).await
    }
}

/// Makes `<root>/mcp` and writes `files` there. Off the actor.
fn write(root: &Path, files: &[File]) -> Result<(), Error> {
    let folder = root.join(FOLDER);
    crate::write::create_private_dirs_under(root, &folder)
        .and_then(|()| {
            files
                .iter()
                .try_for_each(|file| crate::write::replace_private_file(&file.path, &file.bytes))
        })
        .map_err(|error| {
            let error = crate::log_file::Cause(&error);
            tracing::warn!(%error, "an MCP server's launch file was not written");
            Error::refused(
                ErrorCode::AgentLaunchFailed,
                "Clauseo could not write the file a tool you added starts from.",
            )
        })
}

#[cfg(test)]
mod tests;
