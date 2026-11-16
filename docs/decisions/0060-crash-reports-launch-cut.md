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
  on and saving (after the welcome's first act), with no prompt of their own. The welcome's
  counts line says so (UC-750, in place of UD1-08), and Settings, Privacy adds one sentence under
  the counts switch (UC-261). Turning counts off deletes every stored report, and cancels a
  report's POST in flight.
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
  - macOS can't tell a sent SEGV/BUS/ILL from a fault (XNU sets fixed si_codes), so on macOS
    every such signal that reaches the handler is recorded; an outside kill -SEGV leaves one
    report. XNU's `sendsig` (`bsd/dev/arm/unix_signal.c`) gives SIGSEGV `SEGV_ACCERR`, SIGBUS
    `BUS_ADRALN` and SIGILL `ILL_ILLTRP` however the signal came, and on Intel
    (`bsd/dev/i386/unix_signal.c`) a real general protection fault arrives with `si_code` 0.
    The handler records and raises the signal again under the previous action, so the process
    still dies. SIGTERM and SIGKILL are never recorded.
  - Windows: the unhandled-exception filter (`crates/host/src/crash/native/win.rs:32`) takes
    every exception no handler took, a stack overflow (`0xC00000FD`) included; `abort` fails fast
    past every filter, so its own report is the panic hook's. Inside a crashed process the
    filter avoids (H = `crates/host/src/crash`, C = `crates/counts/src`):
    - heap allocation: the line is built in a stack buffer (H `native.rs:66`) by
      `native_line` (C `crash/native.rs:27`), with no `String` or `Vec`;
    - locks, the CRT's included: `OnceLock::get` is an atomic load (H `native.rs:60`,
      H `native/win.rs:39`), and `recording`, `launched` and `begin_crash` are atomics
      (C `crash.rs:137`, `:147`, `:154`); there is no `Mutex`, `malloc` or CRT stdio;
    - file APIs beyond one handle opened at start: one `WriteFile` through `&File`
      (H `native.rs:70`), with no open, flush or path;
    - formatting machinery: no `format!`, `Display` or `tracing`; the digits are written by
      hand (C `crash/native.rs:33`).
    A second fault inside the filter finds this run's one crash taken (`begin_crash`, a
    swap), so it writes nothing. The filter never returns `EXCEPTION_CONTINUE_EXECUTION` itself
    (H `native/win.rs:39`), so nothing loops, and the process ends with the first code.
  - The webview's renderer gone, once per run (a renderer that keeps failing makes one report):
    macOS's `webViewWebContentProcessDidTerminate:` through Tauri's
    builder hook (the page is loaded again, as Tauri does with no hook; no reason given, so
    `other`), WebView2's `ProcessFailed` for a render process that exited (not `TERMINATED`,
    an end from outside), WebKitGTK's `web-process-terminated` for a crash or out of memory (not
    an end the app asked for).
    Real-machine row for WebView2's `ProcessFailed` wiring (`watch`, `reason_of`); the mapping
    itself, `gone_reason` (`crates/desktop/src/counts/crashes/win.rs`), is unit-tested. It gives
    `Crashed` → `crashed`, `OutOfMemory` → `out_of_memory`, `Unresponsive` → `unresponsive`,
    `LaunchFailed` → `launch_failed`, and `Unexpected` (or no reason, on an older runtime) →
    `other`. `Terminated` gives no report, and neither does any kind but a render process that
    exited.
    - (a) End the renderer from outside: `taskkill /PID` on the `msedgewebview2.exe` with
      `--type=renderer`. Expect no report: WebView2 says `Terminated`, which records nothing by
      design.
    - (b) A real renderer crash, with no app code. Expect one `renderer_gone` report with reason
      `crashed`, and no page content or URL beyond what this record allows.
      1. Launch Clauseo with the user env var
         `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`. It still applies:
         WebView2 takes the variable in place of the app's arguments, and `keep_no_dns_in_env`
         (`crates/desktop/src/navigation/browser_args.rs:160`, first in `main`) keeps the flags it
         has and only appends the no-DNS rule.
      2. In Edge, open `edge://inspect`, find the Clauseo target and Inspect it.
      3. In DevTools, turn on Settings › Experiments › Protocol monitor, then send `Page.crash`.
      4. Fallback, if the target is not reachable: an out-of-memory loop in that DevTools
         console. Expect one report with reason `out_of_memory`. If neither is reachable, mark (b)
         NOT RUN with the reason.
  - `area` comes from the panic's file's crate, else the crashing thread's name from a fixed
    list, else `other`; `after_launch` is whether the main window had had the focus.
- **The tally still counts.** Each crash adds one `crash` cell to the weekly tally while counts
  are on, whether or not its report can be sent.
- **Stored, then sent.** Under `<data_dir>/crash/`: at most 5 reports waiting, each kept at most
  30 days. A file is read only when it is a plain file of at most 2 KB, and the native record is
  deleted and made anew (`create_new`), so a link put in its place is never followed. A report
  is read back through `parse_crash` and sent as `crash_body` writes it again, so a file changed
  on disk goes only while it still passes the schema; a report the app wrote goes byte for byte.
  One gzip POST to `/v1/crash` on the counts endpoint, at a random moment 1 to 24 hours after
  the crash and never within an hour of a counts POST, so a report cannot be paired with a day
  report. 204 or 400 deletes the report; 429 (the Worker's hourly cap), 503, any other answer or
  none keeps it for a later try, 1 to 6 hours on, for up to its 30 days.

## Consequences

- A lawyer who keeps counts on now also sends crash reports, and the welcome and Settings say
  so. Firm IT turns them off alone with `privacy.crash_reports: false` (`docs/firm-it.md`,
  section 12).
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
