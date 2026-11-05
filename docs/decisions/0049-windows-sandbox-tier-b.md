# 0049: The Windows sandbox, tier B

**Draft.** S15 PR2 starts this record. Later S15 pull requests complete it: the ACL walk and
ledger (PR1, PR3), set-up (PR4), the runner and token (PR5), the launcher (PR6), the network
(PR7), the install-folder grants (PR8), and Codex (PR9).

## Context

On macOS the agent runs under Seatbelt (tier A, 0038). Windows has no jail of that kind that
Clauseo can use today without admin rights, so Windows has been tier C: Not protected, with the
adapters' own ask modes. S15 adds tier B: one local sandbox user, `ClauseoSandbox`, made once per
machine after one admin prompt on the Safe workspace screen, with one random SID per matter, so
one matter's agent cannot read another matter's folders. The parts that make the user, the
token, the ACL edits and the WFP filters are vendored from Codex's `windows-sandbox-rs`
(Apache-2.0, tag `rust-v0.162.1`).

Tier B works only from a desktop session; from session 0 the restricted child fails with
`0xC0000142` (S11 row 11). The S15 PR0 spikes, on a copy of `clauseo-win-01` (Windows Server
2025, build 26100), changed the token the plan started from:

- `CreateProcessWithLogonW` gives the runner the desktop user's own logon SID, so with that SID
  restricting or in the default DACL, one matter's agent could open another's process with
  `VM_READ` and `TERMINATE`, read the desktop user's processes, and rewrite the window station's
  and desktop's DACLs.
- `0xC0000142` came from the default DACL, not the desktop: the token's own default DACL names
  no restricting SID.
- `Users` must be a restricting SID (System32 DLLs outside KnownDLLs grant `Users` only; without
  it Node fails `0xC0000135`). `Everyone` need not be, and with it any matter reads another
  matter's default-secured pipes.

## Decision

- **Tier B is one sandbox user with one SID per matter.** The runner logs on as `ClauseoSandbox`
  and starts the agent under a restricted token, not write-restricted, so reads are checked too.
  As PR0 settled it:
  - restricting SIDs: the matter's SID, `RESTRICTED` and `Users`;
  - the logon SID deny-only, and not restricting;
  - a narrow default DACL: `ClauseoSandbox`, the matter SID and `SYSTEM` with full access, and
    the desktop user with `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE`
    only;
  - a private desktop per runner (`SYSTEM`, `ClauseoSandbox` and the matter SID), with read and
    execute on `WinSta0` for `ClauseoSandbox` and the matter SID;
  - `SetErrorMode` in the runner, so a failed start exits instead of hanging on an error box.
- **Matter SIDs** are `S-1-5-21-<a>-<b>-<c>-<rid>`, three random sub-authorities and a random
  relative id of at least 1000, from the OS's randomness. Each is made at the matter's first
  launch and kept in `<data_dir>\sandbox\matters.json` (matter id to SID), replaced whole and
  atomically. A matter keeps its SID; only `forget_matter` removes it. A SID is never given to a
  second matter, and a `matters.json` that can't be read is refused, never rewritten, since
  rewriting it would lose the SIDs that name existing permission entries
  (`clauseo_sandbox::windows::sids`).
- **Grants come in pairs**, one ACE for `ClauseoSandbox` (the normal check) and one for the
  matter SID (the restricted check), written ahead in `<data_dir>\sandbox\acl-ledger.json`.
  Nothing is granted in the lawyer's `<profile>\.claude`.
- **The set-up marker** `<data_dir>\sandbox\setup.json` holds the set-up version, the Windows
  user who ran it, the `ClauseoSandbox` SID and the time (`clauseo_sandbox::windows::marker`).
  It reads as missing, older, current or another user's (and as newer or unreadable, which fail).
  The machine's `HKLM\SOFTWARE\Clauseo\Sandbox\UserSid` record names the sandbox user for the
  desktop's start-up check (#317); its reader now lives in
  `clauseo_sandbox::windows::sandbox_user_sid`.
- **Set-up state.** `SandboxSetup::state` gives `Ready` only for a current marker of this user
  whose sandbox SID the `UserSid` record also names; a missing marker or a missing record is
  `Needed`, an older marker `Outdated`, another user's marker or a record naming another account
  `SetUpByOtherUser`, and anything unreadable `Failed`.
- **Detection.** `detect(setup)` caches only what the OS can give: on Windows, the machine's
  architecture (`IsWow64Process2`'s native machine) and build (`CurrentBuildNumber`). ARM64 and
  builds before Windows 10 22H2 (19045) are tier C, `UnsupportedOs`. Otherwise the set-up's state
  is read on every call: tier B only when it is `Ready`, else C with `NeedsSetUp`,
  `SetUpRefused` or `SetUpFailed`. macOS is unchanged: the Seatbelt probe, once per process.

The plan's eight decisions (orchestrator, 2026-11-05):

- **D1.** The folder gate runs in the host and again in `SandboxUserLauncher::new`, on every
  folder it would grant on; a failing data folder makes the machine tier C.
- **D2.** Claude on a `CantChangePermissions` matter on a tier-B machine is plain tier C, with
  no forced ask.
- **D3.** Codex: try its `unelevated` sandbox nested inside tier B first; if that fails, run
  codex-acp's full-access mode with tier B the only fence, never granting `WRITE_DAC`.
- **D4.** The ACE ledger is write-ahead: `pending`, the ACE, `applied`; pending entries are
  reconciled at start.
- **D5.** The first Windows user owns the set-up; a second user's set-up stops with
  `SetUpFailed` ("set up by another Windows user") and that user stays on tier C.
- **D6.** Detection caches only the OS, architecture and MXC probe, and reads the set-up state
  at each tier query, so a finished set-up shows without a restart.
- **D7.** The ACL walk skips only name-surrogate reparse points (junctions, symbolic links);
  cloud placeholders get the ACE through `FILE_FLAG_OPEN_REPARSE_POINT`.
- **D8.** On a matter whose root DACL grants a restricting group read (`Users`, per PR0), every
  other matter's SID gets a ledgered deny-read ACE.

## Consequences

- A finished set-up moves the machine to tier B at the next tier query; R17's
  `RetryProtection` needs no restart.
- Two matters' agents run as one Windows user, so isolation rests on the restricting SIDs and the
  default DACL. HKCU stays readable by every matter (`RESTRICTED` reads the hive).
- A second Windows user on the same machine stays on tier C until machine-scope secrets land.
- The token, the default DACL and the desktop come from the PR0 spikes on Windows Server 2025;
  Windows 11 and Windows 10 22H2 are unmeasured.

## Status

proposed (draft; S15's later pull requests complete it)

## Date

2026-11-05
