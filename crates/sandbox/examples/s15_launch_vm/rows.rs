//! The rows: each a function from the prepared harness to [`Outcome`]. [`run_row`] runs one on a
//! thread of its own under a hard limit: a row past it is `FAIL <row>: timed out after N s`, and
//! every process the harness started and hasn't waited for is ended (`launch::kill_started`).

use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use super::checks;
use super::launch::kill_started;
use super::manifest::Manifest;
use super::sys::{now, say};

/// Every row, in the order `rows` and `--unattended` run them.
pub(crate) const ROWS: [&str; 23] = [
    "whoami",
    "node-pipes",
    "open-process",
    "pipes-5",
    "job",
    "stdio",
    "exit-code",
    "codex",
    "ca-bundle",
    "clipboard",
    "atoms",
    "runners",
    "lawyer-processes",
    "breakaway",
    "handles",
    "desktop",
    "pipes-same-matter",
    "m3-hardlink",
    "pool-processes-at-start",
    "pool-processes-at-end",
    "sweep-before-logon",
    "sweep-mid-session",
    "agent-token-config",
];

/// How long the watchdog waits past a row's own limit before it calls the row hung.
const GRACE: Duration = Duration::from_secs(15);

/// A row's result.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// It held; a note on what was seen.
    Pass(String),
    /// It didn't; why.
    Fail(String),
    /// An input it needs is missing; which.
    Skip(String),
    /// The agent's side held, but a positive control failed, so it proves nothing: why, and the
    /// note. Neither a pass nor a fail.
    Info(String),
}

/// What a row works with.
pub(crate) struct Ctx<'a> {
    /// The manifest prepare wrote.
    pub(crate) manifest: &'a Manifest,
    /// The row's limit; every wait in it ends by then.
    pub(crate) limit: Duration,
    /// The row's name, for its notes.
    pub(crate) row: &'a str,
    /// `--interactive-desktop`: `node-pipes` needs each UI effect proven (`gate.rs`).
    pub(crate) strict: bool,
    /// Why the row can be no more than INFO (a positive control failed); `None` while none has.
    pub(crate) info: Mutex<Option<String>>,
}

impl Ctx<'_> {
    /// One note line under the row.
    pub(crate) fn note(&self, line: &str) {
        say(&format!("  {}: {line}", self.row));
    }

    /// Marks the row INFO at best: a positive control failed (`why` names the call and code).
    /// A later failure still fails it.
    pub(crate) fn info(&self, why: &str) {
        self.note(&format!("INFO: {why}"));
        if let Ok(mut held) = self.info.lock() {
            held.get_or_insert_with(|| why.to_owned());
        }
    }

    /// The outcome of the row's `result`: a fail stays a fail; a pass is INFO when [`Ctx::info`]
    /// was called.
    pub(crate) fn outcome(&self, result: Result<String, String>) -> Outcome {
        let info = self.info.lock().ok().and_then(|held| held.clone());
        match (result, info) {
            (Err(why), _) => Outcome::Fail(why),
            (Ok(note), Some(why)) => Outcome::Info(format!("{why}; the agent's side: {note}")),
            (Ok(note), None) => Outcome::Pass(note),
        }
    }
}

/// Runs the row `name` under `limit` and prints its verdict line; its outcome. `strict` is
/// `--interactive-desktop`.
pub(crate) fn run_row(name: &str, manifest: &Manifest, limit: Duration, strict: bool) -> Outcome {
    say(&format!("row {name}: start (limit {} s)", limit.as_secs()));
    let started = now();
    let (send, receive) = mpsc::channel();
    let owned = manifest.clone();
    let row = name.to_owned();
    std::thread::spawn(move || {
        let ctx = Ctx {
            manifest: &owned,
            limit,
            row: &row,
            strict,
            info: Mutex::new(None),
        };
        drop(send.send(body(&ctx)));
    });
    let outcome = match receive.recv_timeout(limit + GRACE) {
        Ok(outcome) => outcome,
        Err(RecvTimeoutError::Timeout) => {
            let ended = kill_started();
            Outcome::Fail(format!(
                "timed out after {} s (ended the harness's processes {ended:?}, pid and code)",
                started.elapsed().as_secs()
            ))
        }
        Err(RecvTimeoutError::Disconnected) => Outcome::Fail("the row's thread died".to_owned()),
    };
    let seconds = started.elapsed().as_secs_f32();
    match &outcome {
        Outcome::Pass(note) => {
            say(&format!("  {name}: {note} ({seconds:.1} s)"));
            say(&format!("PASS {name}"));
        }
        Outcome::Fail(why) => say(&format!("FAIL {name}: {why} ({seconds:.1} s)")),
        Outcome::Skip(why) => say(&format!("SKIP {name}: {why}")),
        Outcome::Info(why) => say(&format!("INFO {name}: {why} ({seconds:.1} s)")),
    }
    outcome
}

fn body(ctx: &Ctx<'_>) -> Outcome {
    if ctx.manifest.pool_pair().is_none() {
        return Outcome::Fail("the manifest names no two pool users: run prepare".to_owned());
    }
    let result = match ctx.row {
        "whoami" => checks::whoami(ctx),
        "node-pipes" => {
            if ctx.manifest.node.is_none() {
                return Outcome::Skip("needs --node at prepare".to_owned());
            }
            checks::node_pipes(ctx)
        }
        "open-process" => checks::open_process(ctx),
        "pipes-5" => checks::pipes(ctx),
        "job" => checks::job(ctx),
        "stdio" => checks::stdio(ctx),
        "exit-code" => checks::exit_code(ctx),
        "codex" => return super::codex::row(ctx),
        "ca-bundle" => checks::ca_bundle(ctx),
        "clipboard" => {
            return match super::checks_ui::clipboard(ctx) {
                Ok(Some(note)) => Outcome::Pass(note),
                Ok(None) => {
                    Outcome::Skip("no clipboard the harness can use here (above)".to_owned())
                }
                Err(why) => Outcome::Fail(why),
            };
        }
        "atoms" => super::checks_ui::atoms(ctx),
        "runners" => super::checks_sec::runners(ctx),
        "lawyer-processes" => super::checks_sec::lawyer_processes(ctx),
        "breakaway" => super::checks_sec::breakaway(ctx),
        "handles" => super::checks_sec::handles(ctx),
        "desktop" => super::checks_sec::desktop(ctx),
        "pipes-same-matter" => super::checks_sec::pipes_same_matter(ctx),
        "m3-hardlink" => super::checks_m3::m3_hardlink(ctx),
        "pool-processes-at-start" => super::checks_pool::at_start(ctx),
        "pool-processes-at-end" => super::checks_pool::at_end(ctx),
        "sweep-before-logon" => super::checks_pool::before_logon(ctx),
        "sweep-mid-session" => super::checks_pool::mid_session(ctx),
        "agent-token-config" => super::checks_pool::token_config(ctx),
        other => Err(format!("no row {other:?}; rows: {}", ROWS.join(", "))),
    };
    ctx.outcome(result)
}
