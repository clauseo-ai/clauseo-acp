//! `clauseo-process`: starts an agent with stdio pipes in a process tree of its own, stops it by a
//! ladder, and kills the whole tree.
//!
//! # Owns
//!
//! - Every agent process start: [`spawn_in_tree`] is the one place a child starts, and sandbox
//!   launchers later rewrite the spec and call it, behind the same [`Launcher`] trait.
//! - The process tree: a new process group on Unix (`process_group(0)`, signals through `nix`'s
//!   `killpg`); on Windows a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, which the agent
//!   joins while suspended (`CREATE_SUSPENDED`, then its threads resume) and which it never runs
//!   outside. Windows children get `CREATE_NO_WINDOW`. Every Win32 call is in `os/windows.rs`.
//! - The shutdown ladder: close stdin, 2 s, SIGTERM to the group, 3 s, SIGKILL to the group. On
//!   Windows: close stdin, 2 s, end the job.
//! - The stderr ring (256 KiB per agent, never uploaded) and its redacted tail of at most 4 KiB.
//!
//! # Interface
//!
//! - [`LaunchSpec::new`]: an absolute program (never a `.cmd` or `.bat`), its arguments, the
//!   variables set on top of the inherited environment, and an absolute working folder.
//! - [`Launcher`] (`host` holds an `Arc<dyn Launcher>`) and [`PassthroughLauncher`], which calls
//!   [`spawn_in_tree`] with the spec unchanged and its [`SpawnOptions`] (the home folder for
//!   redaction, and on Windows extra creation flags).
//! - [`Launched`]: [`AgentStdin`] (an `AsyncWrite` the handle can close), the agent's stdout, the
//!   [`StderrPump`] the caller must drive, and the [`Handle`].
//! - [`Handle::exited`] (the agent exited on its own; its tree is then killed),
//!   [`Handle::shutdown`] (the ladder, with a [`Timer`]: [`AsyncIoTimer`], or with the
//!   `test-support` feature `ManualTimer`), and `Drop` (kills the tree, no waits).
//! - [`ExitReport`]: the code or signal, [`ExitReport::stderr_tail`], and
//!   [`ExitReport::detach_reason`] for `AgentSessionDetached`. [`ShutdownOutcome`] names the
//!   ladder step that ended the agent.
//! - [`Error`], every failed launch, with [`Error::code`] (`agent.launch_failed`).
//!
//! # Invariants
//!
//! - No shell: never `sh -c`, `cmd.exe`, `npx` or a batch file, and no `PATH` lookup.
//! - Executor-neutral: no spawn, no sleep, no clock. The caller drives every future, the stderr
//!   pump included; [`AsyncIoTimer`] is the only real timer, and it is the caller's choice.
//! - However the agent ends (stdin closed, signal, crash, or the handle dropped), what is left of
//!   its tree is killed. An empty group is fine.
//! - Only the redacted tail leaves the crate; the ring is never logged or returned whole. The tail
//!   reads no variable: the home folder comes in [`SpawnOptions`], the secrets from the spec.
//! - A running agent never makes an [`Error`]: how it ended is an [`ExitReport`]. The crate
//!   appends no event and reads no setting; `host` turns reports into events.
//!
//! # Depends on
//!
//! `protocol` (`ErrorCode`, `DetachReason`), `async-process`, `async-io` (the real timer),
//! `futures`, `memchr` (line ends in the stderr pump), `thiserror`; `nix` on Unix (`signal`) and
//! `windows` on Windows. Never `tauri` or `tokio`.
//!
//! # Start reading
//!
//! `launch.rs` (the spawn), then `handle.rs` (exit and the ladder), `os/unix.rs` and
//! `os/windows.rs` (the trees), `stderr.rs` (the pump, the ring and the tail) and `stdin.rs`.
//! Tests: unit tests in `src/*/tests.rs`; the pipe tests against `fake_agent` in `tests/pipes/`.

mod error;
mod handle;
mod launch;
mod os;
mod spec;
mod stderr;
mod stdin;
mod timer;

pub use error::Error;
pub use handle::{ExitReport, Handle, ShutdownOutcome};
pub use launch::{Launched, Launcher, PassthroughLauncher, SpawnOptions, spawn_in_tree};
pub use spec::LaunchSpec;
pub use stderr::StderrPump;
pub use stdin::AgentStdin;
#[cfg(feature = "test-support")]
pub use timer::ManualTimer;
pub use timer::{AsyncIoTimer, Timer};
