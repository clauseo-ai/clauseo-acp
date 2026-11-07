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
  information.
- `lawyer-processes`: the same rights, refused on the lawyer's processes: the outer runner, the
  host (the harness itself, standing in for Clauseo.exe) and the proxy (a `probe wait` the harness
  starts as the lawyer, standing in for the egress proxy's process). A granted
  `PROCESS_QUERY_LIMITED_INFORMATION` there fails the row and is named, for a decision.
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

## In CI

The `s15-launch` workflow, which lives on the throwaway branch `exp-pr5-harness` and never on
main, runs `s15_launch_vm --unattended --node <node> --codex <dir>` on `windows-11-arm` and
`windows-2025`. Unattended mode runs prepare, every row and then cleanup,
and cleanup runs even when a row fails. It prints one `PASS`, `FAIL` or `SKIP` line per row, and
exits 1 when prepare or any row failed. The output is the job's artifact.

## Probes

`s15_launch_vm probe <what>` is what the runner starts inside the sandbox: `token`, `wait`,
`open-process <pid>`, `open-pipes <name>...`, `spawn-child`, `echo`, `exit <n>`, `read <path>`,
`write <path>`, `job-ui`, `clipboard-read`, `clipboard-write <text>`, `find-atom <name>`,
`runners`, `breakaway`, `handles`, `desktop`. `probe.rs` and `probe_sec.rs` document what each
prints.
