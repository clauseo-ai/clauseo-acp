# 0010: process dependencies

## Context

`process` starts each agent, shuts it down and kills its whole process tree. The crates table
listed only `protocol` and `async-process` under "May depend on". The crate must run on whatever
executor the host gives it, without spawning a task or naming `tokio`, must signal a process
group on Unix, and must put the agent in a Job Object on Windows before it runs. That needs:

- `async-io` 2: the timer behind the shutdown ladder's 2 s and 3 s waits. It drives its timers
  from its own thread, so it needs no particular executor. `async-process` already depends on it.
- `futures` 0.3.32: the `AsyncWrite` the agent's stdin implements, the `AsyncRead` the stderr
  pump reads, and `select` between the agent's exit and a wait.
- `memchr` 2.8: finds the line ends in each 16 KiB stderr read, instead of a byte-by-byte scan.
- `nix` 0.31.3 (`signal`), Unix only: `killpg` sends SIGTERM and SIGKILL to the agent's process
  group through a safe wrapper, so the crate has no `unsafe` on Unix.
- `windows` 0.62.2, Windows only: the Job Object (`CreateJobObjectW`,
  `SetInformationJobObject`, `AssignProcessToJobObject`, `TerminateJobObject`), the process
  handle that joins the job, and the Toolhelp walk that resumes the suspended agent. The root
  line's features are the ones `xtask` already turns on; `process` needs no others.

- `libc` 0.2.191, macOS only (added for clauseo-acp-private#359): libproc's
  `proc_listchildpids`, `proc_pidinfo(PROC_PIDTBSDINFO)` and `proc_pidpath`, which list the
  agent's descendants with their start times. Linux reads `/proc` with safe std, and Windows
  reads the Job Object's process list (`QueryInformationJobObject`) and each process's parent
  (a Toolhelp process snapshot) with the `windows` features already on. The root `libc` line is
  shared with `desktop`. A sweep's cutoff reads the clock process starts are stamped with:
  `nix`'s `time` (`clock_gettime`) and `feature` (`sysconf(_SC_CLK_TCK)`) on Unix, and
  `GetSystemTimeAsFileTime` (`windows`' `Win32_System_SystemInformation`, on `process`'s own
  line) on Windows.

The table lists a crate's normal dependencies other than `thiserror`.

## Decision

The `process` row's "May depend on" cell is `protocol, async-process, async-io, futures, memchr;
windows (Windows only); nix (Unix only); libc (macOS only)`, and its status is `exists`. "Never depends on" stays
`tauri`. Its dev-dependencies (itself with `test-support`, `clauseo-fake_agent`, `serde_json`,
and `nix` with `process` and `event` for the tests' exit watch) are not listed, as for every
crate.

## Consequences

`process` stays free of any executor: the host drives the stderr pump, the exit wait and the
shutdown ladder on its own. Its only `unsafe` is in the Windows module and the macOS listing
(`os/macos.rs`, one wrapper per libproc call), each behind one `#[expect(unsafe_code)]`. A bump of the root `nix` or `windows` line now reaches both `xtask`
and `process`, so it is checked against both.

## Status

accepted

## Date

2026-10-28
