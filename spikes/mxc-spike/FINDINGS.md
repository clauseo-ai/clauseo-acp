# MXC spike: findings

Branch `exp-mxc-spike` (throwaway, never merged). G6 part two. Sources: MXC's docs, the
`mxc-sdk` 1.0.1 crate source and the issue tracker (2026-11-07), Clauseo's code, G6 part one,
and the probe run on GitHub's runners. Files: `probe.ps1` (step 1), `rows.ps1` with
`proxy-mock.mjs`, `inside.js` and `acp-driver.mjs` (step 2), `RUNBOOK.md` (real turns on a
Windows 11 machine), workflow `.github/workflows/mxc-spike.yml`.

## Step 1: the probe (run 42341399286)

| | `windows-11-arm` | `windows-2025` |
|---|---|---|
| OS | Windows 11 25H2 Enterprise (registry ProductName says "Windows 10 Enterprise"), **26200.9725**, ARM64, Client | Server 2025 Datacenter 24H2, 26100.33674, x64, Server |
| Documented floor (26200.9278) | met | not listed (Server) |
| `processmodel.dll`, PSEC exports | present, all three in `processmodel.dll` | missing |
| `wxc-exec --probe` | `base-container`, no DACL changes; deny paths, enumerate paths, host-loopback ingress and identity-less loopback proxy all supported | `appcontainer-dacl`, needs DACL changes |
| cmd inside, UI off / on | OK 136 ms / OK 98 ms | not run |
| node inside, UI on | **OK, 192 ms** (3,046 ms outside: a cold first start) | not run |
| node inside, UI off | **`0xC0000142`** (microsoft/mxc#1629) | not run |
| Codex's strict MXC check | `MXC_OK` | "native MXC is unavailable on this executor" |
| Caller | session 2, High integrity (the runner is an elevated admin in a desktop session) | session 2, High |

**Tier A can't serve Windows Server or RDS hosts.** Server 2025 has no base container at any UBR
(here 26100.33674; G6: 26100.33650), only MXC's AppContainer-plus-DACL fallback, which S15a
excludes. MXC documents Windows 11 client builds only, so a multi-session RDS host (Server)
stays on tier B or C.

The 28 s start-up in #1629 did not show on the runner (cmd 98 ms, node 192 ms with UI on).

## Step 2: the rows (`rows.ps1`, `rows` job on `windows-11-arm`)

Everything runs through `wxc-exec.exe` v1.0.1 with UI on, proxy-only egress (Model 2, identity-less
loopback proxy), the install read-only, the matter and state read-write. The layout copies
Clauseo's installed one: `%LOCALAPPDATA%\ai.clauseo.desktop-mxcspike` with
`agents\runtime\node-v24.22.0-win-arm64` (the pinned zip and SHA-256), an `npm ci` of
`crates/limits/tests/live` (claude-agent-acp 0.89.1 with claude-agent-sdk 0.3.293's `claude.exe`,
codex-acp 2.2.2 with codex 0.162.1's ARM64 `codex.exe`), `agents\<id>\home`, a state folder, and a
matter under `Documents`. Two filesystem shapes:

- **strict:** no `enumeratePaths`, nothing denied.
- **A:** `enumeratePaths` on each drive root (the workaround from #1676), `Documents` denied (the
  matter is a read-write child of it, which stays writable under BaseContainer's overlap rule).

Rows: 0 Node starts; 1 realpath (#1464); 2 Git Bash (#1061); 3 `del` and `unlinkSync` (#1676);
4 DNS (#1666), direct TCP refused, HTTPS through the proxy, whether Clauseo's proxy token
survives, `curl.exe` through the environment's proxy; 5 reads outside refused, the tree
cmd → node → ping killed when `wxc-exec` is killed, start-up inside versus outside; 6
`claude --version` and `codex --version` inside, one `claude -p` turn (Bash then Write) and one
`codex exec` turn (shell) against the scripted model server, and one ACP turn through each adapter
(`node.exe` plus `dist/index.js`, Clauseo's launch shape).

**The model server.** `proxy-mock.mjs` is the CONNECT proxy and also answers `/v1/messages`
(Anthropic) and `/v1/responses` (OpenAI) on the same loopback port. The agents can reach only the
proxy endpoint, so it has to be the same port. Before the push, all four turns were checked on the
Mac against the same pinned packages (darwin builds), outside any sandbox:
- `claude -p` wrote both files, three model calls;
- `codex exec` asked for `exec_command`;
- the claude-agent-acp turn ended `end_turn` with 2 permission requests and both files;
- the codex-acp turn ended `end_turn`.

## What S15a and G6 already cover, and what is new

| Item | Already there | New in this spike |
|---|---|---|
| Tier A design | 0038: tier A on Windows is MXC; S15a plans `processcontainer` through `wxc-exec.exe` behind Codex's PSEC probe (G6 row 1) | nothing |
| Tier A code | **none**: `crates/sandbox/src/detect.rs:127` says "Tier A (S15a's MXC probe) is not built"; ARM64 is tier C until tested (`detect.rs:131`, G6 row 13) | the probe runs G6's checks; the rows drive `wxc-exec` from PowerShell, as G6's scripts did. No Clauseo example exists to drive |
| Server 2025 | G6 row 1: no `processmodel.dll` | confirmed on the `windows-2025` runner, 26100.33674 |
| Windows 11 | G6 rows 2 to 10 pending (no machine) | the free `windows-11-arm` runner qualifies; rows 2 to 9 in their no-login form run there |
| ARM64 | G6 row 13 pending | first tier-A data on ARM64 |
| ACP over stdio, tree kill | G6 row 5: a kill-on-close job without breakaway ends the whole tree (tier B and MXC T3) | the same under tier A (row 5c, 6d) |

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

## Workarounds and fix dates for the issues the rows test

| Issue | State (2026-11-07) | Workaround | Fix date |
|---|---|---|---|
| [#1464](https://github.com/microsoft/mxc/issues/1464) realpath | open, no assignee | nilleb, 2026-10-27: grant the container read access to `\GLOBAL??`, each drive's symbolic link, `\GLOBAL??\MountPointManager` and `\\.\MountPointManager`. That changes object ACLs machine-wide and needs admin, so it isn't tier A's "no admin" | none given |
| [#1061](https://github.com/microsoft/mxc/issues/1061) Git Bash | open, no assignee | xoviat, 2026-10-29: a fix in the Cygwin/MSYS runtime (one missing ACE), on a fork (github.com/xoviat/cygwin/tree/mxc); not upstream, waiting for an MSYS2 maintainer (2026-11-02) | none given |
| [#1676](https://github.com/microsoft/mxc/issues/1676) del | open, no assignee | djlsystems, 2026-11-06: `enumeratePaths: ["C:\\"]` makes `del`, `rmdir /s`, `Remove-Item` and `node script.js` work; enumeration is recursive, so names (not contents) become listable everywhere not denied. Shape A tests exactly this | none given |
| [#1666](https://github.com/microsoft/mxc/issues/1666) DNS | open, no comments | none | none given |
| [#1629](https://github.com/microsoft/mxc/issues/1629) UI | open | keep UI on (`ui.disable: false`); the 28 s delay did not reproduce on the runner | none given |

## Not verified

- Every row of step 2 until the `rows` job has run.
- A desktop user's Medium token: the runner is elevated (High). `RUNBOOK.md` covers it on a
  Windows 11 machine, with real logins.
- x64 Windows 11: no free runner. The ARM64 results may not carry over.
