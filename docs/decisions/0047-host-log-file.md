# 0047: A local log file for the host

## Context

The host and the crates under it have about 280 `tracing` sites (warn, error and info), but no
build installed a subscriber and no panic hook was set, so every event and every panic message
was dropped on every OS. On Windows the app runs in the GUI subsystem with no stderr at all.
Runs that failed on a lawyer's machine left nothing to read. The log must not become a second
copy of the matter: a log holding paths, file names or prompts would itself be matter content.

## Decision

- **Where.** `host.log` in `Locations::log_dir()`, which is `<data folder>/logs` and follows
  `CLAUSEO_DATA_DIR`:
  - Windows: `%LOCALAPPDATA%\ai.clauseo.desktop\logs\host.log` (the data folder is the
    LocalAppData known folder joined with the app identifier; `%LOCALAPPDATA%\Clauseo` is the
    install folder, not this one);
  - macOS: `~/Library/Application Support/ai.clauseo.desktop/logs/host.log`;
  - Linux: `$XDG_DATA_HOME/ai.clauseo.desktop/logs/host.log` (`~/.local/share/...` when unset).
  The file stays on the machine: never uploaded, never read back by the app.
- **The cap.** At 5 MB `host.log` becomes `host.1.log` and `host.1.log` becomes `host.2.log`,
  so three files and 15 MB at most. The rotation is written by hand (`tracing-appender` rotates
  by time only); the host gets `tracing-subscriber` with its `fmt` feature, already in the lock.
- **What.** Warn and error from every target. Info only from a `clauseo_*` crate's
  `::lifecycle` target (`clauseo_host::lifecycle`): the log's start, the host's start and stop,
  an agent's launch (thread id, agent id, pid) and end (exit code or close reason), a matter's
  open and close by id. Debug and trace never. Each line: UTC RFC 3339 time, level, target,
  message and fields. A panic hook writes each panic's message, `file:line` of the code and
  thread name, then runs the hook set before it.
- **The content rule: ids, codes and kinds only.** Allowed: matter, thread, turn, stream and
  session ids; error codes and kinds; durations, counts, exit codes and pids; agent ids, names
  and versions; non-identifying categories (the path kind such as local, UNC, mapped or
  removable; the filesystem and drive type; the sandbox tier). Never: a path, a file or folder
  name, a prompt, agent output (stderr tails included), a title, a host the agent reached, or a
  settings value. An error's text can carry a path (`matters::Error::Io` holds one), so a host
  site logs `error = %Cause(&error)`, which prints the `ErrorCode`s along the source chain and
  each `io::Error`'s kind, never the text; `ProfileError::log_text` does the same for the
  profile's errors. A test runs host scenarios with the file on and fails if the log holds the
  matter's folder, its file names, the prompt, the agent's text or the thread's title. A panic
  message is code text (the crates ban `panic!`, `unwrap` and `expect` outside tests), so it is
  written as it is.
- **Backpressure.** A caller formats its line and hands it to a bounded channel (1024 lines)
  with `try_send`. A full channel drops the line and counts it; no thread ever waits for the
  disk. One `clauseo-log` thread writes, in batches, and puts `dropped N lines` before the next
  line it writes after a drop.
- **Failure.** Nothing in the log ever panics, fails the host or blocks a caller.
  - The folder cannot be made, the thread cannot start, or a subscriber is already set:
    `install` returns a guard that holds nothing, and nothing is logged.
  - A write fails (a full disk, an I/O error): its lines are dropped and counted, and the disk
    is left alone for 5 s before the next try.
  - A rotation fails (on Windows, another program holding `host.log` open): writing stops at
    the cap rather than growing past it, lines are dropped and counted, and the rotation is
    tried again after 5 s.
  - The panic hook writes on a helper thread and waits for it 250 ms at most.
  - The guard's drop, at exit, waits 500 ms at most for the queued lines.
- **Who installs it.** The app calls `clauseo_host::log_file::install(&locations)` once, first
  thing in `run()`, keeps the guard until exit, and calls `LogGuard::flush` before an exit
  that skips drops (`std::process::exit`).
- **Uninstall.** On Windows the NSIS uninstaller's ticked "Also delete conversations, saved
  versions and settings" box already removes `logs\` with the rest of the data folder (Tauri's
  template runs `RmDir /r $LOCALAPPDATA\${BUNDLEID}`), for the current user only and never a
  `CLAUSEO_DATA_DIR` override. On macOS and Linux there is no removal step, so the logs stay with
  the rest of the data folder. No installer change is needed.
- **Not logged.** The webview's console (the UI's errors) does not reach this file; that would
  be an app follow-up through a narrow command. Crates that do not use `tracing` (`log`-only
  dependencies) are not bridged.

## Consequences

- A failed run leaves a file a lawyer can send, holding nothing of the matter.
- Every new warn or error site must follow the content rule: no `path = %…`, and an error goes
  through `Cause` (or a `log_text`), never `%error` of a type whose text holds a path. Review
  enforces it; the content test catches what its scenarios reach.
- A full disk or a held file costs log lines, never host time. The cost on the caller is one
  format and one `try_send` per kept event; filtered events cost a cached callsite check.
- Sites in `crates/limits` and `crates/codex_appserver` (frozen for this change) and in
  `crates/desktop` (the app's) that log a path or an error's text are listed in the pull request
  that added the file, for their owners to change.

## Status

proposed

## Date

2026-11-04
