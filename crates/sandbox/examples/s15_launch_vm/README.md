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

Rows: `whoami`, `node-pipes`, `open-process`, `pipes-5`, `job`, `stdio`, `exit-code`, `codex`,
`ca-bundle`, `clipboard`, `atoms`, `runners`, `lawyer-processes`, `breakaway`, `handles`, `desktop`,
`pipes-same-matter`, `m3-hardlink`. `clipboard` skips in a session with no clipboard the harness
can use. `node-pipes` needs `--node`; `codex` needs `--node` and `--codex`, both given to
prepare, which copies them into `bin\`. Each row has a limit (`--row-timeout`, 120 s by default).
A row past it fails, and the harness ends every process it started.

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
- `breakaway`: `CREATE_BREAKAWAY_FROM_JOB` fails with 5, or its child is still in a job; the
  child is killed either way.
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

The UI limits (`whoami`, `node-pipes` and `codex` check them; `checks_ui.rs`): the agent runs
on a window station of its own, `ClauseoRun-…`, whose DACL grants the pool user `GA`, in a job
with every `JOB_OBJECT_UILIMIT_*` (`0xff`). The probe's direct read of its innermost job's UI
limits must be exactly `0xff`. Only when it reads exactly `0x0` (node's children sit in
libuv's own nested job) does the effect decide instead, and both halves must hold:

- `CreateDesktopW` on its own station fails with exactly 5. The station's DACL grants
  `WINSTA_CREATEDESKTOP`, so only the job's `DESKTOP` limit refuses it.
- The clipboard write fails in `OpenClipboard`, `EmptyClipboard` or `SetClipboardData` with
  5 (or `0x80070005`), what `WRITECLIPBOARD` gives. The call that failed is printed.

Then the harness runs the positive control (`checks_ctl.rs`): `probe ui-control` once per
variant, each on its own fresh window station `ClauseoS15Ctl-<hex>` made as the outer runner
makes `ClauseoRun-…` (`CreateWindowStationW(name, 0, GENERIC_ALL, sa)`, a DACL of `SYSTEM`, the
token's user and the harness's user, labelled Low, with a `Default` desktop), kept open while
the process runs on `ClauseoS15Ctl-<hex>\Default`. No variant is in a job of ours; each inherits
whatever job the harness runs in (on CI, the runner service's), which the row notes. The probe
tries `CreateDesktopW` on its own station, then the clipboard write, and prints each result with
its token's user, integrity and restricting SIDs. Every variant's result is printed. The
variants, closest to the agent first:

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

The control is the first variant, in that order, that ran on its `ClauseoS15Ctl-` station with
its own token shape and whose desktop create and clipboard write both succeeded; the row notes
how it differs from the agent. The gate: only a variant at (d) or closer lets the row pass,
since then the agent's 5s come from the job's limits. When (e) or (f) is the first to succeed,
the row is `INFO` with every variant's codes as evidence, for a decision (for (f) the note says
the control is unrestricted). When none succeeds, the row is `INFO`, naming each variant's
codes, never `PASS`. Any other read of the agent's limits fails the row and names the value.

`node-pipes` and `codex` also add a global atom as the lawyer and pass its name to
`probe job-ui <atom>` (as node's argument, and in the command the mock asks codex to run). What
the agent's lookup and the harness's own lookup get is printed as information, never the pass
rule: atoms belong to a window station, so the agent's `ClauseoRun-…` station hides the lawyer's
atom whatever the job's limits are. The lookup outside the job is the control for the harness's
own atom; the desktop refusal is the effect that tells the limits apart.

The unit test of that rule (`checks_ui.rs`) runs on Windows with
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

`s15_launch_vm probe <what>` is what the runner starts inside the sandbox: `token`, `wait`,
`open-process <pid>`, `open-pipes <name>...`, `spawn-child`, `echo`, `exit <n>`, `read <path>`,
`write <path>`, `job-ui [atom]`, `clipboard-read`, `clipboard-write <text>`, `find-atom <name>`,
`runners`, `breakaway`, `handles`, `desktop`; and `ui-control` and `ui-relay`, which the
harness starts itself (the positive control, above). `probe.rs` and `probe_sec.rs` document
what each prints.
