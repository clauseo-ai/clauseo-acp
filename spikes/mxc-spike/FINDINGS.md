# MXC spike: findings so far

Branch `exp-mxc-spike` (throwaway, never merged). G6 part two, step 1: the probe. Everything below
comes from reading MXC's docs, the `mxc-sdk` 1.0.1 crate source and the issue tracker
(2026-11-07), plus Clauseo's code and G6 part one. Nothing has run on Windows yet. The probe
workflow (`.github/workflows/mxc-spike.yml`, script `probe.ps1`) is what runs first.

## What S15a and G6 already cover, and what is new

| Item | Already there | New in this spike |
|---|---|---|
| Tier A design | 0038: tier A on Windows is MXC; S15a plans `processcontainer` through `wxc-exec.exe` behind Codex's PSEC probe (G6 row 1) | nothing |
| Tier A code | **none**: `crates/sandbox/src/detect.rs:127` says "Tier A (S15a's MXC probe) is not built"; ARM64 is tier C until tested (`detect.rs:131`, G6 row 13) | no Clauseo probe exists to run, so the workflow runs the checks G6 used: `processmodel.dll`, `wxc-exec --probe`, Codex's strict `windows.sandbox = "mxc"` check |
| Server 2025 | G6 row 1: no `processmodel.dll` at 26100.33650, tier `appcontainer-dacl` | `windows-2025` runner reruns it as a control |
| Windows 11 | G6 rows 2 to 10 pending (no machine) | `windows-11-arm` runner: does a free runner meet the floor? |
| ACP over stdio, tree kill | G6 row 5: a kill-on-close job without breakaway around the launcher ends the whole tree (tier B and MXC T3) | MXC itself puts the child in a kill-on-close job, created suspended (see below) |

No new launcher was written. A first draft of a standalone launcher on the `mxc-sdk` crate was
dropped before commit once the orchestrator pointed to S15a's `wxc-exec` plan.

## What MXC's API offers against each need

| Need | What MXC 1.0.1 does | Source |
|---|---|---|
| Folder grants | `readwritePaths`, `readonlyPaths`, `deniedPaths` (native deny needs `PSE_SUPPORT_FS_DENY`). A grant covers the folder and its descendants, except a volume root, which does not cascade | [schema.md, Filesystem Policy](https://github.com/microsoft/mxc/blob/main/docs/schema.md#filesystem-policy) |
| Deny by default | Yes, plus whatever `ALL APPLICATION PACKAGES` can already reach (Windows, Program Files and the like). An explicit deny beats that group | [filesystem.md](https://github.com/microsoft/mxc/blob/main/docs/backends/process-container/filesystem.md) |
| Ancestors (#1109) | Closed by `processContainer.filesystem.enumeratePaths` (PSEC 1.1, `fs_enumerate`). Older 25H2 builds refuse it: "enumeratePaths is not supported by this version of Windows" | [#1109](https://github.com/microsoft/mxc/issues/1109), [#1162](https://github.com/microsoft/mxc/issues/1162) |
| Proxy-only egress | `runtimeConfig.networkProxy` must be loopback; WFP pins the container to that address and port. An unpackaged proxy needs `ingress.default: allow` and `hostLoopback: allow`, which the docs call "development/testing"; the strong form needs a packaged or AppContainer proxy (`allowedProxyPeer`) | [networking.md](https://github.com/microsoft/mxc/blob/main/docs/backends/process-container/networking.md) |
| Env passed through | Yes (`process.env`, `inheritDefaultEnv`), but SYSTEMROOT and LOCALAPPDATA are required, PSEC may replace LOCALAPPDATA, and with a proxy MXC **removes** caller `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, `ALL_PROXY` (any case) and sets a bare `http://127.0.0.1:<port>` | `appcontainer_runner.rs:86, 238-272` in the crate; schema.md "Environment" |
| Tree kill | The child is created suspended, put in a job with `KILL_ON_JOB_CLOSE`, then resumed; `kill()` is `TerminateJobObject` plus a wait for the job to empty. Launch fails closed if the job can't be made | `base_container_runner.rs:985-1065, 1531-1550` |
| Node | Runs only with UI on (`ui.disable: false`); then the first output takes ~28 s on 25H2 .9711. UI off: Node exits `0xC0000142` | [#1629](https://github.com/microsoft/mxc/issues/1629) (open) |
| OS floor | Windows 11 24H2/25H2 26100/26200.9278, 26H2 26300.9550, 26H1 28000.2804. Client SKUs only (G6: Server lacks it) | [os-version-support.md](https://github.com/microsoft/mxc/blob/main/docs/backends/process-container/os-version-support.md) |

## Issues that bear on running the whole agent inside

- **[#1464](https://github.com/microsoft/mxc/issues/1464) (open): `GetFinalPathNameByHandleW(VOLUME_NAME_DOS)` always fails in BaseContainer.** Node's `fs.realpathSync.native` and `fs.promises.realpath` fail with `EPERM`; Git for Windows can't get its working directory. One commenter reports **GitHub Copilot CLI itself cannot run inside a BaseContainer** because of it. This is the nearest existing test of "the whole Node agent inside".
- **[#1629](https://github.com/microsoft/mxc/issues/1629) (open):** ~28 s before first output with UI on; Node fails with UI off.
- **[#1465](https://github.com/microsoft/mxc/issues/1465) (open):** from session 0 every USER32 program (node, pwsh, whoami) fails `0xC0000142`; cmd and curl run. Matters for CI runners and services, not for Clauseo on a desktop.
- **[#1676](https://github.com/microsoft/mxc/issues/1676), [#1599](https://github.com/microsoft/mxc/issues/1599) (open):** `cmd del` and `Remove-Item` are denied in `readwritePaths`; Node's `unlinkSync`, .NET delete and rename work. Shell tools the agents run would hit it.
- **[#1061](https://github.com/microsoft/mxc/issues/1061) (open):** MSYS2/Cygwin programs (git-bash) cannot start; Claude Code's Bash tool on Windows uses Git Bash.
- **[#1666](https://github.com/microsoft/mxc/issues/1666) (open):** DNS lookups still work in proxy mode, through the host's DNS client: a data-exfiltration channel.
- **[#1548](https://github.com/microsoft/mxc/issues/1548) (open):** matters on folder-mounted volumes fail (Node `realpathSync` EPERM).
- **[#1648](https://github.com/microsoft/mxc/issues/1648) (open):** PSEC create returns `0x80070005` after a clean probe on one 26100 machine.
- Clauseo's earlier AppContainer failure (0049, spike A: pipes only under `\\.\pipe\LOCAL\`, Node spawn hangs) is not reported against BaseContainer; reports of Node agent hosts running under `wxc-exec` with UI on (#1629 comments) suggest Node's pipes work there. Unverified.

## Proxy: Clauseo's own proxy does not fit as is

`crates/sandbox/src/proxy.rs` puts a token in `HTTPS_PROXY`'s userinfo and checks it on the agent
listener. MXC strips that variable and sets a bare endpoint, so the token never reaches the
agent. Under MXC the per-container WFP pin does the token's job (a container can reach only its
own proxy port), so tier A would need the agent listener without the token check. It also binds
a random port each launch, which is fine for MXC (the port goes in `networkProxy`).

## Not verified

Everything on Windows. The probe answers the first question (does a free runner meet the floor);
the rows (rw matter and state, read outside refused, direct network refused, proxy working, tree
killed, start-up time, the whole agents inside) wait on it.
