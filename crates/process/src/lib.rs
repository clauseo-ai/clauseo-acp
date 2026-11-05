//! `clauseo-process`: starts an agent with stdio pipes in a process tree of its own, stops it by a
//! ladder, kills the whole tree, and lists and ends what the agent started.
//!
//! # Owns
//!
//! - Every agent process start: [`spawn_in_tree`] is the one place a child starts, and sandbox
//!   launchers later rewrite the spec and call it, behind the same [`Launcher`] trait.
//! - The process tree: a new process group on Unix (`process_group(0)`, signals through `nix`'s
//!   `killpg`); on Windows a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, which the agent
//!   joins while suspended (`CREATE_SUSPENDED`, then its threads resume) and which it never runs
//!   outside. Windows children get `CREATE_NO_WINDOW`. Every Win32 call is in `os/windows.rs`.
//! - The shutdown ladder: close stdin, 2 s, SIGTERM to the group, 3 s, SIGKILL to the group, and
//!   2 s more for the exit. On Windows: close stdin, 2 s, end the job, 2 s. On Unix the tree kill
//!   then kills every descendant in the listing the caller passed to the shutdown, because a
//!   command an agent starts in a session of its own (Codex does) has left the group.
//! - The agent's descendants: listed by parent pid on Unix (`/proc` on Linux, libproc on macOS)
//!   and from the job's process list on Windows (parents from Toolhelp), each with its start
//!   time so a reused pid is never signalled; and the sweep that ends those new since an earlier
//!   listing and started before a cutoff, with what they start meanwhile.
//! - The processes running under a name, with their starts (macOS and Windows), for `matters`'
//!   stale owner-file rule: every process from libproc's `proc_listallpids` on macOS, from a
//!   Toolhelp snapshot on Windows.
//! - The stderr ring (256 KiB per agent, never uploaded) and its redacted tail of at most 4 KiB.
//!
//! # Interface
//!
//! - [`LaunchSpec::new`]: an absolute program (never a `.cmd` or `.bat`), its arguments, the
//!   variables set on top of the inherited environment, and an absolute working folder.
//! - [`Launcher`] (`host` holds an `Arc<dyn Launcher>`) and [`PassthroughLauncher`], which calls
//!   [`spawn_in_tree`] with the spec unchanged and its [`SpawnOptions`] (the home folder for
//!   redaction, [`SpawnOptions::clear_env`] so the child gets only the spec's variables, and on
//!   Windows extra creation flags). By default the child inherits this process's environment.
//! - [`Launched`]: [`AgentStdin`] (an `AsyncWrite` the handle can close), the agent's stdout, the
//!   [`StderrPump`] the caller must drive, and the [`Handle`].
//! - [`Handle::exited`] (the agent exited on its own; its tree is then killed),
//!   [`Handle::shutdown`] (the ladder, with a [`Timer`]: [`AsyncIoTimer`], or with the
//!   `test-support` feature `ManualTimer`), and `Drop` (kills the tree, no waits).
//! - [`Handle::descendants`]: a [`Descendants`] that outlives the handle and keeps nothing alive.
//!   [`Descendants::snapshot`] lists them now (a [`Snapshot`], blocking);
//!   [`Descendants::end_new`] ends what is new since a snapshot and started before a [`Cutoff`]
//!   (with what those start meanwhile), but executables the caller keeps (SIGTERM, 2 s, SIGKILL
//!   on Unix; `TerminateProcess`, 2 s, again on Windows), and says what it did ([`Swept`]).
//!   [`Handle::shutdown`] takes the snapshot the caller listed (where it chose) for its kill.
//!   With `test-support`, [`running`] says whether a pid still runs, [`started_before_now`]
//!   whether a cutoff taken now would cover it, and [`kill`] kills one, for tests that watch a
//!   process they did not start.
//! - [`processes_named`] (macOS and Windows): the starts of the running processes with a name,
//!   or `None` when that is unknown. It blocks, and signals nothing.
//! - [`ExitReport`]: the code or signal, [`ExitReport::stderr_tail`], and
//!   [`ExitReport::detach_reason`] for `AgentSessionDetached`. [`ShutdownOutcome`] names the
//!   ladder step that ended the agent, or says it never exited (`Unresponsive`).
//! - [`Error`], every failed launch, with [`Error::code`] (`agent.launch_failed`).
//!
//! # Invariants
//!
//! - No shell: never `sh -c`, `cmd.exe`, `npx` or a batch file, and no `PATH` lookup.
//! - Executor-neutral: no spawn, no sleep, no clock. The caller drives every future, the stderr
//!   pump included; [`AsyncIoTimer`] is the only real timer, and it is the caller's choice. The
//!   descendant listings block; the caller decides where they run, the shutdown's included (it
//!   passes the listing in). The one clock read is [`Cutoff::now`], the clock process starts
//!   are stamped with, read only to compare with those starts.
//! - A process is signalled or ended only after its pid's start time is read again and matches
//!   the listing's. A failed listing is never read as "no descendants", so it never makes every
//!   process look new.
//! - However the agent ends (stdin closed, signal, crash, or the handle dropped), what is left of
//!   its tree is killed: its group or job, and after a shutdown the listing it was given. An empty
//!   group is fine.
//! - Only the redacted tail leaves the crate; the ring is never logged or returned whole. The tail
//!   reads no variable: the home folder comes in [`SpawnOptions`], the secrets from the spec.
//! - A running agent never makes an [`Error`]: how it ended is an [`ExitReport`]. The crate
//!   appends no event and reads no setting; `host` turns reports into events.
//!
//! # Depends on
//!
//! `protocol` (`ErrorCode`, `DetachReason`), `async-process`, `async-io` (the real timer),
//! `futures`, `memchr` (line ends in the stderr pump), `thiserror`; `nix` on Unix (`signal`),
//! `libc` on macOS (libproc) and `windows` on Windows. Never `tauri` or `tokio`.
//!
//! # Start reading
//!
//! `launch.rs` (the spawn), then `handle.rs` (exit and the ladder), `os/unix.rs` and
//! `os/windows.rs` (the trees), `descendants.rs` with `os/linux.rs` and `os/macos.rs` (the
//! listings and the sweep), `stderr.rs` (the pump, the ring and the tail) and `stdin.rs`.
//! Tests: unit tests in `src/*/tests.rs`; the pipe tests against `fake_agent` in `tests/pipes/`.

mod descendants;
mod error;
mod handle;
mod launch;
#[cfg(all(test, any(target_os = "macos", windows)))]
mod named_tests;
mod os;
mod spec;
mod stderr;
mod stdin;
mod timer;

pub use descendants::{Cutoff, Descendants, Snapshot, Swept};
pub use error::Error;
pub use handle::{ExitReport, Handle, ShutdownOutcome};
pub use launch::{Launched, Launcher, PassthroughLauncher, SpawnOptions, spawn_in_tree};
pub use spec::LaunchSpec;
pub use stderr::StderrPump;
pub use stdin::AgentStdin;
#[cfg(feature = "test-support")]
pub use timer::ManualTimer;
pub use timer::{AsyncIoTimer, Timer};

/// The start times, as wall time, of the running processes named `name`: the executable's
/// file name on macOS (`Microsoft Word`, from libproc's `proc_pidpath`, byte for byte),
/// the image name on Windows (`WINWORD.EXE`, ignoring ASCII case). An empty list when none runs.
/// `None` when the answer is unknown: the listing failed, or a process that may be one could not
/// be read. One listing of every process, and one path read per process and one more read per
/// match (macOS) or one read per match (Windows). macOS and Windows only; `matters` asks it for
/// Word (its stale owner-file rule).
#[cfg(any(target_os = "macos", windows))]
pub fn processes_named(name: &str) -> Option<Vec<std::time::SystemTime>> {
    os::named(name)
}

/// Whether process `pid` still runs (a Unix zombie does not), for tests that watch a process
/// they did not start.
#[cfg(feature = "test-support")]
pub fn running(pid: u32) -> bool {
    os::running(pid)
}

/// Whether process `pid` started before a [`Cutoff`] taken now (a sweep ends only what did), for
/// tests that must wait out the clock's unit (10 ms on Linux) before they take one.
#[cfg(feature = "test-support")]
pub fn started_before_now(pid: u32) -> bool {
    os::started_before_now(pid)
}

/// Kills process `pid` (SIGKILL; `TerminateProcess` on Windows), for a test that cleans up a
/// process its agent started after a failed assert. Only for a pid the test's own agent started.
#[cfg(feature = "test-support")]
pub fn kill(pid: u32) {
    os::kill_pid(pid);
}
