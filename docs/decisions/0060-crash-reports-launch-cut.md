# 0060: Crash reports, the launch cut of S22

## Context

Task S22 plans full crash reporting: minidumps walked on the machine by a crash-handler child
process, a consent card for each report, a sent list, a typed log with its scrubber, stall
detection and a symbols job. None of it is built, and launch is close. Until now a crash left
only a `crash` cell in S21's weekly tally ([decision 0036](0036-counts.md)): it says that the app
crashed, never where. Rohan's ruling (2026-11-15): crash reports go live before launch as a
launch cut of S22, with no minidumps, no crash-handler child process, no consent card, no
symbols job and no new crates.

## Decision

- **Crash reports follow the counts.** A report is made and sent only while `privacy.counts` is
  on and saving (after the welcome's first act), with no prompt of their own. Settings, Privacy
  adds one sentence under the counts switch (UC-261). Turning counts off deletes every stored
  report.
- **`privacy.crash_reports` is firm IT's lock alone.** The off-only security key from 0036 gets
  no UI. A policy `false` turns crash reports off while counts stay on, and deletes every stored
  report; an unreadable policy source locks it off, as for every security key. The lawyer's own
  value of the key is not used.
- **A closed report.** `clauseo_counts::schema::CrashReport` (`crates/counts/src/schema/crash.rs`):
  `schema`, its own random UUID v4 `report` (not the install id, not the tally id), the ISO
  `week` (no date or time), `version`, `channel`, `os`, `os_major`, `arch`, `process` (`host`,
  `webview`), `kind` (`panic`, `signal`, `renderer_gone`), `area` (one of Clauseo's crates by
  name, `webview` or `other`) and `after_launch`; then per kind a panic's `location` (Clauseo's
  own source file `crates/<crate>/src/...rs` or a dependency's `<dep>-<version>/src/...rs`, and
  the line; left out for any other file), a native crash's `code` (the signal number on macOS
  and Linux, the exception code on Windows), or the renderer's `reason` (`crashed`,
  `out_of_memory`, `unresponsive`, `terminated`, `launch_failed`, `other`; macOS gives none, so
  `other`). Never document or matter content, names, prompts, paths on the lawyer's machine, the
  panic message, the install id, a time finer than the week, memory, stack frames or logs.
- **Which crashes.** One panic or native report per run, from the host's start thread on (a
  crash while the settings and the launch profile load is not recorded).
  - Panics: every panic of a build that aborts on panic (the release profile's `panic =
    "abort"`), where each one ends the app. The host's panic hook makes the report on the
    panicking thread and writes it on its helper thread, waiting at most 750 ms. A build that
    unwinds may catch a panic and carry on, so it records none.
  - Native crashes, caught in process (`crates/host/src/crash/native`): `sigaction` for
    `SIGSEGV`, `SIGBUS`, `SIGILL` and `SIGFPE` raised by the kernel for a fault, and `SIGABRT`
    (on Linux, not one another process sent); on Windows the unhandled-exception filter. The
    handler appends only the code to a file opened at start, with async-signal-safe calls, then
    the previous handler runs. A user kill, a force quit, a power loss or a Windows shutdown
    raises none of these, so it never makes a report, and the `SIGABRT` that follows a panic is
    the panic's report. The next start turns the record into a report, with the version that
    crashed.
  - The webview's renderer gone: macOS's `webViewWebContentProcessDidTerminate:` through Tauri's
    builder hook (the page is loaded again, as Tauri does with no hook; no reason given, so
    `other`), WebView2's `ProcessFailed` for a render process that exited (not `TERMINATED`,
    an end from outside), WebKitGTK's `web-process-terminated` for a crash or out of memory (not
    an end the app asked for).
  - `area` comes from the panic's file's crate, else the crashing thread's name from a fixed
    list, else `other`; `after_launch` is whether the main window had had the focus.
- **The tally still counts.** Each crash adds one `crash` cell to the weekly tally while counts
  are on, whether or not its report can be sent.
- **Stored, then sent.** Under `<data_dir>/crash/`: at most 5 reports waiting, each kept at most
  30 days; the bytes stored at the crash are the bytes sent, before gzip. One gzip POST to
  `/v1/crash` on the counts endpoint, at a random moment 1 to 24 hours after the crash and never
  within an hour of a counts POST, so a report cannot be paired with a day report. 204 or 400
  deletes the report; 429, 503, any other answer or none keeps it for a later try.

## Consequences

- A lawyer who keeps counts on now also sends crash reports, and Settings says so. Firm IT turns
  them off alone with `privacy.crash_reports: false` (`docs/firm-it.md`, section 12).
- A report says where Clauseo stopped (a crate, a source line, a signal or a reason) but carries
  no stack, so a crash deep in a dependency is found only as far as its file and line.
- A new field or value is a schema change, as for the counts (0036): the Worker's `/v1/crash`
  must read it through `parse_crash` before a release sends it.
- 0036's `privacy.crash_reports` stays in the schema and `SECURITY_KEYS`, default `false`, but
  only a policy value is read. Full S22 may make it the lawyer's own switch again.
- **Deferred to full S22:** minidumps and stack frames walked on the machine (the
  crash-handler/minidumper child process and the `clauseo-crash` crate); the consent card and the
  sent list (UF); the protocol wire (`CrashReportReady`, `GetCrashReport`, `AnswerCrashReport`,
  `ListSentCrashReports`); the typed log and its scrubber; `stall` detection (host and webview
  heartbeat) and JS error reports; the `symbols` job and `clauseo-crashes` symbolication; S21's
  canary test with a forced crash; the `crash_test` feature and deliberate-crash runs per OS; and
  `privacy.crash_reports` as a lawyer's own switch with UI.

## Status

proposed

## Date

2026-11-15
