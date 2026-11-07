# s15_launch_vm

The S15 PR5 harness: it runs tier B's runner (`clauseo-sandbox-run.exe`) the way the host will,
and checks what the agent gets. It starts an agent as a matter's pool user under the restricted
Low token. Windows only.

## Build

```text
cargo build -p clauseo-sandbox --bins --example s15_launch_vm --target <native target>
```

The harness looks for `clauseo-sandbox-run.exe` and `clauseo-sandbox-setup.exe` beside itself
and one folder up (cargo puts examples in `examples\`). `--runner` and `--setup` override that.

## On a VM copy (step mode, session 1)

Run each step in the copy's desktop session through `run2.sh`, never over SSH's session 0. Put
each step in a one-line `.ps1`, for example
`& C:\s15\s15_launch_vm.exe prepare --node C:\node\node.exe --codex C:\codex\node_modules\@openai`.

1. `prepare`: under `Highest` when the copy has no pool yet (prepare runs the set-up), else
   `Limited` (prepare uses the pool the machine's record names, in that record's data folder).
2. `rows`, or `row <name>` for one row: `Limited`.
3. `cleanup`: the same token prepare had.

Each step prints what it did. Prepare writes `C:\ClauseoS15Vm\s15-manifest.txt` (`--root`
moves it). Cleanup deletes only the paths listed there, and runs `clauseo-sandbox-setup --remove`
only when prepare ran the set-up.

## VM rows

- **Interactive desktop** (`--interactive-desktop`): first candidate carrying PR 5; an
  interactive desktop session (console or RDP, not session 0); run the harness with
  `--interactive-desktop`; the agent's node child is refused CreateDesktopW, SwitchDesktop and
  clipboard writes. CreateDesktopW and the clipboard write each succeed for variant (g), the
  agent's token shape on the agent's own window station outside the runner's job. SwitchDesktop
  succeeds for variant (f)'s token on WinSta0 (the control `w`); that pair differs in station as
  well as job, so it shows the agent can't switch the user's desktop, not the job's limit alone.

  In the copy's console or RDP session (not over SSH's session 0), from the copy's own folder
  (not `C:\ClauseoS15Vm\bin`, which prepare fills and cleanup deletes):

  ```text
  C:\s15\s15_launch_vm.exe --unattended --node C:\node\node.exe
      --codex C:\codex\node_modules\@openai --interactive-desktop
  ```

  (one line), or after `prepare`, `C:\s15\s15_launch_vm.exe row node-pipes --interactive-desktop`.
  With the flag, `node-pipes` fails, naming the call and code, unless (g) succeeded at
  CreateDesktopW and at the clipboard write and the agent's node child was refused each, and (w)
  switched desktops while the agent's child didn't; (a) to (f) carry nothing there. Without it
  (CI), `SwitchDesktop` is information only and one effect carried by (g), or by (a) to (d)
  when (g) didn't run, is enough (below).

## The rows

Rows: `whoami`, `node-pipes`, `open-process`, `pipes-5`, `job`, `stdio`, `exit-code`, `codex`,
`ca-bundle`, `clipboard`, `atoms`, `runners`, `lawyer-processes`, `breakaway`, `handles`, `desktop`,
`pipes-same-matter`, `m3-hardlink`, `pool-processes-at-start`, `pool-processes-at-end`,
`sweep-before-logon`, `sweep-mid-session`, `agent-token-config`. `clipboard` skips in a session
with no clipboard the harness can use. `node-pipes` needs `--node`; `codex` needs `--node` and
`--codex`, both given to prepare, which copies them into `bin\`. Each row has a limit
(`--row-timeout`, 120 s by default). A row past it fails, and the harness ends every process it
started.

The security rows (`checks_sec.rs`, `checks_m3.rs`):

- `runners`: A's agent is refused `PROCESS_ALL_ACCESS`, `PROCESS_VM_READ`,
  `PROCESS_DUP_HANDLE`, `PROCESS_QUERY_INFORMATION`, `PROCESS_QUERY_LIMITED_INFORMATION` and
  the token (`TOKEN_QUERY`, `TOKEN_DUPLICATE`) on its inner runner (its parent pid) and its outer
  runner (the harness sends that pid on stdin). What `SYNCHRONIZE` gets is printed as
  information. Once the agent prints its parent, the harness, as the lawyer outside the job,
  lists every thread of both runners (`TH32CS_SNAPTHREAD`, by owner pid) and sends the ids on
  stdin (`threads <role> <tid>...`, with the pids, before `end`); the agent's own snapshot
  sees none of them under its token, so its count is only printed as information. The agent
  tries `OpenThread` with `THREAD_ALL_ACCESS`, `THREAD_QUERY_LIMITED_INFORMATION` and
  `THREAD_GET_CONTEXT` on each id: every open must be refused (any code but 0), the thread
  count per runner is printed, and a live runner with no thread id from the harness fails (it
  proves nothing; so does one whose every open gave 87, gone). The harness's own
  process snapshot (`TH32CS_SNAPPROCESS`) finds any `conhost.exe` whose parent is either
  runner and sends each as `conhost <pid>` in the same write; the agent must be refused the
  same process and token rights on each. `conhost none (harness snapshot)` is printed when there
  is none, as information: it is expected, since both runner exes are GUI-subsystem. What the
  agent's own snapshot finds is printed as information too.
- `lawyer-processes`: the same rights, refused on the lawyer's processes: the outer runner, the
  host (the harness itself, standing in for Clauseo.exe) and the proxy (a `probe wait` the harness
  starts as the lawyer, standing in for the egress proxy's process), and the runners' threads and
  console hosts as in `runners`. A granted `PROCESS_QUERY_LIMITED_INFORMATION` there fails the
  row and is named, for a decision.
- `breakaway`: `CREATE_BREAKAWAY_FROM_JOB` fails with 5, or its child is still in the session's
  own kill-on-close job; the child is killed either way. While the probe waits (it reads one
  stdin line before the kill), the harness duplicates that job's handle from the outer runner's
  handle table (the one job there that kills on close and has every UI limit; the outer makes it
  unnamed) and asks `IsProcessInJob(child, job)`, or reads the job's pid list when it can't open
  the child. A child in another job (any job but ours) or in none fails. The harness closes its
  copy of the handle before the session ends, so the job still closes with the outer.
- `handles`: the agent's handle table holds exactly the `in`, `out` and `err` runner pipes,
  never `ctl` (by name; by counting client pipe ends when names don't come back).
- `desktop`: prints the DACLs of `WinSta0` and `WinSta0\Default`; the agent runs on its own
  `ClauseoRun-…` window station, can't open `WinSta0` (and if it could, can't open `Default`
  there after `SetProcessWindowStation`), and `SwitchDesktop` gives 5 under the job's desktop
  UI limit (without that limit the row fails, `no desktop UI limit`). An ACE for A's pool SID
  on `Default` prints `ALERT desktop: …` and fails the row. Without the limit, `SwitchDesktop`
  may switch the session to the agent's empty desktop, so run it on a VM copy only.
- `pipes-same-matter`: a second agent of A's matter gets 5, not 231, on each of A's pipes.
- `m3-hardlink`: makes `<root>\m3\profile\.codex\auth.json` (a fixture) and links to it
  from A's state folder. `codex_link_in_policy` with that stand-in profile refuses the hard
  link (`LinkedLogin`) and a symbolic link (`ReparseLink`); the outer runs the hard link (it
  isn't the real `auth.json`) and refuses the symbolic link and a link in the real profile's
  `.codex` (never made). The symbolic link half skips without developer mode.

The pool rows (`checks_pool.rs`, verdicts in `pool_verdict.rs`) watch from outside that no
process of matter A's pool user runs outside the session's kill-on-close job: what the runner's
process sweep does (it kills each such process before the pool user's logon, before the agent's
runner resumes, every 30 s and at the session's end). Each session runs `probe hold`, which
ends at its stdin's end, as a host's agent does. The harness lists the pool user's processes
itself: a Toolhelp snapshot, then `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` and
`OpenProcessToken(TOKEN_QUERY)` on each, comparing the token's user to the pool SID; the job is
the outer's, duplicated as in `breakaway`. Processes it can't open are counted as information.

- `pool-processes-at-start`: once the agent printed its first line, every listed process is in
  the job (`IsProcessInJob`); FAIL names each that isn't, pid and image. INFO when the listing
  didn't see the agent itself (it can't see the pool user's processes).
- `pool-processes-at-end`: as above while the session runs (the agent must be seen), then the
  listing, retried for up to 10 s after the session ended, is empty; FAIL names what's left.
- `sweep-before-logon`: before the session, the harness starts `probe wait` as A's pool user
  (the user the launch message names, from the manifest), outside any job, with
  `CreateProcessWithLogonW` as the positive control's relay starts (`win_logon.rs`), on a
  control station of its own; once it printed its pid, the session starts. PASS when that
  process is gone by the agent's first line. INFO when it never ran.
- `sweep-mid-session`: the same process, started once the agent printed its first line, must
  be gone within 45 s of its start.
- `agent-token-config`: the states of Secondary Logon (`seclogon`), WMI (`Winmgmt`), Task
  Scheduler (`Schedule`) and BITS (`QueryServiceStatus`), as information that never decides the
  row; then `probe token` as A: PASS only when the restricting SIDs are exactly the pool user,
  `S-1-5-12`, `S-1-5-32-545` and `S-1-1-0`, the integrity is Low and every logon SID is
  deny-only (as `whoami` asserts); FAIL names each that differs; INFO when the token can't be
  read. The logon SID's attributes are printed.

The sweep rows fail until the runner's sweep lands: they are its test. The harness ends its
benign process if it still runs at the row's end, and notes it.

The UI limits (`whoami`, `node-pipes` and `codex` check them; `checks_ui.rs`): the agent runs
on a window station of its own, `ClauseoRun-…`, whose DACL grants the pool user `GA`, in a job
with every `JOB_OBJECT_UILIMIT_*` (`0xff`). The probe's direct read of its innermost job's UI
limits must be exactly `0xff`. Only when it reads exactly `0x0` (node's children sit in
libuv's own nested job) does the effect decide instead, and both halves must hold:

- `CreateDesktopW` on its own station fails with exactly 5. The station's DACL grants
  `WINSTA_CREATEDESKTOP`, so only the job's `DESKTOP` limit refuses it. Agent and control both
  ask for a 128 KB heap (`CreateDesktopExW`): with the default heap every control variant, the
  lawyer's plain token included, got 8 in CI's session 0. The default-heap code is printed too,
  as information.
- The clipboard write fails in `OpenClipboard`, `EmptyClipboard` or `SetClipboardData` with
  5 (or `0x80070005`), what `WRITECLIPBOARD` gives. The call that failed is printed.
- `SwitchDesktop` (`win_switch.rs`): the probe makes a desktop (the small heap), switches to it,
  switches straight back to the input desktop (or its thread's own) and closes it. The code is
  printed, or the create's code when the create failed (`create-failed <code>`). Only an
  interactive window station can switch, so outside `--interactive-desktop` it is information.

The gate is decided per effect (`gate.rs`). An effect is discriminating when a control that
counts succeeded at it. The row passes when the agent's child is refused (the codes above) at
least one discriminating effect; it fails when the agent's child succeeds at an effect (the limit
is missing) or gets another code at a discriminating one; it is `INFO` only when no effect is
discriminating. An effect no control could carry is named in the row text as information
(`desktop: no control could create one ((a) 8, …), information only`), and the text names the
variant that carried each discriminating effect. On CI every variant gets 8 from
`CreateDesktopW`, while their clipboard writes succeed, so the clipboard carries the row.

Then the harness runs the positive control (`checks_ctl.rs`): `probe ui-control` once per
variant, each on its own fresh window station `ClauseoS15Ctl-<hex>` made as the outer runner
makes `ClauseoRun-…` (`CreateWindowStationW(name, 0, GENERIC_ALL, sa)`, a DACL of `SYSTEM`, the
token's user and the harness's user, labelled Low, with a `Default` desktop), kept open while
the process runs on `ClauseoS15Ctl-<hex>\Default`. No variant is in a job of ours; each inherits
whatever job the harness runs in (on CI, the runner service's), which the row notes. The probe
tries `CreateDesktopW` on its own station, then the clipboard write, and prints each result with
its token's user, integrity and restricting SIDs. Every variant's result is printed. The
variants, closest to the agent first:

- (g) the agent's own token shape on the agent's own station, outside the runner's job, so only
  the job differs. In `node-pipes`, node prints `node waiting` after `probe job-ui` (which
  prints `ui-effect station ClauseoRun-<hex>`) and waits for a line on stdin, so the agent's
  station still exists. The harness starts `probe ui-relay` as matter A's pool user, as for (a),
  on a fresh control station, and the relay starts `probe ui-control` with the agent's token
  shape on `ClauseoRun-<hex>\Default` (the station's DACL names the pool user). (g) counts only
  when it ran on that station, by the name the agent printed, as pool user A, Low, with 4
  restricting SIDs. When (g) ran, it alone can carry an effect; when it couldn't, the row text
  says why (the call and its code) and (a) to (d) carry instead. `codex` and `whoami` skip
  (g): their probe has ended, and the agent's station with it, before the harness reads it.

- (a) the agent's own token: the harness reads matter A's pool user's password from
  `secrets.bin` as the outer runner does (`secrets::secret_for`, never printed) and starts
  `probe ui-relay` as that user with `CreateProcessWithLogonW` (`win_logon.rs`), as the outer
  starts the inner. The relay makes the agent's token from its own as the inner runner's `make`
  does (restricting the pool user, `S-1-5-12`, `S-1-5-32-545`, `S-1-1-0`; the logon SID
  deny-only; Low; `DISABLE_MAX_PRIVILEGE`; the agent's default DACL) and starts
  `probe ui-control` with `CreateProcessAsUserW`. It differs from the agent in not being in the
  runner's job (also: its station's DACL adds the harness's user, and the token object's own
  DACL isn't set).
- (b) the same shape made from the harness's own (the lawyer's) token (`win_ctl.rs`, restricting
  the lawyer instead of the pool user, default DACL the lawyer and `SYSTEM`). A restricted copy
  of the caller's own primary token needs no privilege to assign.
- (c) as (b), with `CreateDesktopW` given explicit `SECURITY_ATTRIBUTES` (a DACL of the token's
  user and `SYSTEM`), in case the default descriptor is what gives 8.
- (d) as (b), restricted but at the lawyer's own integrity level, not Low.
- (e) as (b), Low but with no restricting SIDs.
- (f) the lawyer's own token unchanged, run as a child process the same way.

A variant counts only when it ran on its station (`ClauseoS15Ctl-`, or the agent's for (g)) with
its own token shape. Of (a) to (f), only (d) or closer can carry an effect, since then the
agent's refusal comes from the job's limits; what (e) and (f) succeed at is printed as evidence
only. The row notes how each carrier differs from the agent. Any other read of the agent's
limits fails the row and names the value.

`node-pipes` and `codex` also add a global atom as the lawyer and pass its name to
`probe job-ui <atom>` (as node's argument, and in the command the mock asks codex to run). What
the agent's lookup and the harness's own lookup get is printed as information, never the pass
rule: atoms belong to a window station, so the agent's `ClauseoRun-…` station hides the lawyer's
atom whatever the job's limits are. The lookup outside the job is the control for the harness's
own atom; the desktop refusal is the effect that tells the limits apart.

The unit tests of those rules (`gate.rs`, `checks_ctl.rs`, `checks_ui.rs`, and the pool rows'
and `breakaway`'s in `pool_verdict.rs`) run on Windows with
`cargo test -p clauseo-sandbox --example s15_launch_vm`.

## In CI

The `s15-launch` workflow, which lives on the throwaway branch `exp-pr5-harness` and never on
main, runs `s15_launch_vm --unattended --node <node> --codex <dir>` on `windows-11-arm` and
`windows-2025`. Unattended mode runs prepare, every row and then cleanup, and cleanup runs
even when a row fails. Each row's verdict line starts with `PASS `, `FAIL `, `SKIP ` or `INFO `
(`INFO`: a positive control couldn't run, so the row proves nothing). The last line is
`result: green (…)` or `result: red (…)`, naming every `FAIL` and `INFO` row (and
`FAIL prepare` or `cleanup left something`), and the run exits 1 when it is red: an `INFO` row
turns CI red. `SKIP` doesn't. Step mode (`row <name>`, `rows`) prints the same last line and
exits 1 on `INFO` too. The output is the job's artifact.

## Probes

`s15_launch_vm probe <what>` is what the runner starts inside the sandbox: `token`, `wait`, `hold`,
`open-process <pid>`, `open-pipes <name>...`, `spawn-child`, `echo`, `exit <n>`, `read <path>`,
`write <path>`, `job-ui [atom]`, `clipboard-read`, `clipboard-write <text>`, `find-atom <name>`,
`runners`, `breakaway`, `handles`, `desktop`; and `ui-control` and `ui-relay`, which the
harness starts itself (the positive control, above). `probe.rs` and `probe_sec.rs` document
what each prints.
