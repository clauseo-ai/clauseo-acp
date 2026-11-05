# 0049: The Windows sandbox, tier B

**Draft.** S15 PR2 starts this record and PR1 brings it to design (B). Later S15 pull requests
complete it: the ACL walk (PR3), set-up and the pool (PR4), the runner and token (PR5), the
launcher (PR6), the network (PR7), the install-folder grants (PR8), and Codex (PR9).

## Context

On macOS the agent runs under Seatbelt (tier A, 0038). Windows has no jail of that kind that
Clauseo can use today without admin rights, so Windows has been tier C: Not protected, with the
adapters' own ask modes. S15 adds tier B, set up once per machine after one admin prompt on the
Safe workspace screen, so one matter's agent can't read or write another matter's folders. The
parts that make users, tokens, ACL edits and WFP filters are vendored from Codex's
`windows-sandbox-rs` (Apache-2.0, tag `rust-v0.162.1`).

Tier B works only from a desktop session; from session 0 the restricted child fails with
`0xC0000142` (S11 row 11). The spikes ran on copies of `clauseo-win-01` (Windows Server 2025,
build 26100):

- **PR0** started from one shared user, `ClauseoSandbox`, with one SID per matter. It showed that
  `CreateProcessWithLogonW` hands the runner the desktop user's own logon SID, that `Users` must be
  a restricting SID (System32 DLLs outside KnownDLLs grant `Users` only), and that the default
  DACL, not the desktop, caused `0xC0000142`.
- **That token was superseded** (spike B, private `measurements/s15-spikes-userpool.md`). NPFS
  gives a pipe made with no security set a default descriptor that grants its owner full access
  and `Everyone` read. Under the shared user, the owner wasn't a restricting SID, so the agent
  couldn't open its own pipes for writing: Node's spawn with pipes, and codex-acp starting codex,
  failed. With one pool user per matter, the owner is the pool user, a restricting SID, so both
  work.

## Decision

- **D9: one pool user per matter.** Set-up makes a pool of local standard users (`ClauseoSbNN`,
  in `Users`) and the local group `ClauseoSandboxUsers`, which holds them all; growing the pool
  needs another admin prompt. Each matter is assigned one pool user. The runner logs on as that
  user with `CreateProcessWithLogonW`, which needs the Secondary Logon service: without it the
  logon fails with error 1058, and set-up reports `SetUpFailed` with that cause. The agent runs
  under a restricted token, not write-restricted, so reads are checked too:
  - restricting SIDs: the pool user, `RESTRICTED`, `Users` and `Everyone` (D11), and the
    matter's SID only when a launch asks for it (off by default; spike B found it adds nothing);
  - the logon SID deny-only, and not restricting;
  - Low integrity, with a Low no-write-up label, `S:(ML;OICI;NW;;;LW)`, on the matter, state and
    notes folders, the Claude store and the credential link, so the token writes nothing labelled
    higher;
  - a narrow default DACL: the pool user and `SYSTEM` with full access, and the desktop user with
    `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE` only;
  - a private desktop per runner, labelled Low, and `SetErrorMode` in the runner.
- **Grants** name the matter's pool user: `Modify` on the matter, state and notes folders and the
  credential link, `ReadAndExecute` on runtime and read paths, `CreateFiles` on the Claude store
  folder and `Modify` on its allowed names; a deny of writes on the policy's read-only paths.
  Nothing is granted in the lawyer's `<profile>\.claude`, on the profile or above it, or on
  `<data_dir>\sandbox`; inside `<data_dir>` only an allow list is granted.
- **Isolation between matters** (replacing D8, below):
  - every registered matter's fence: on its matter, state and notes folders, an inherited deny
    of reads and writes for every other pool user, planned at the first launch that finds it
    unfenced, and by set-up and matter creation;
  - on each folder beside the matter in its parent (walked down to any registered matter there),
    and on the files directly in the parent, an inherited deny of reads for
    `ClauseoSandboxUsers`; the parent's own listing still shows names.
- **The ledger** `<data_dir>\sandbox\acl-ledger.json` lists every entry and label, write-ahead
  (D4), one entry per matter even when two matters share an entry, so removal waits for the
  last.
- **Records.**
  - `HKLM\SOFTWARE\Clauseo\Sandbox`, with an explicit DACL: every pool user's SID, which the
    desktop's start-up check (#317) reads through `clauseo_sandbox::windows::sandbox_user_sid`,
    and `OwnerSid`, the Windows user who owns the set-up (D5; `sandbox_owner_sid`). PR2 ships
    the readers, of the values `UserSid` and `OwnerSid`; PR4's set-up writes the pool's SIDs.
  - The local group `ClauseoSandboxUsers`, holding every pool user (PR4).
  - `<data_dir>\sandbox\matters.json`, replaced whole and atomically and refused, never
    rewritten, when it can't be read (`clauseo_sandbox::windows::matters`): today each matter's
    random `S-1-5-21-<a>-<b>-<c>-<rid>` SID, optional under D9 (`windows::sids`); PR4 adds
    each matter's assigned pool user.
  - `<data_dir>\sandbox\setup.json`, the set-up marker: the set-up version, the Windows user
    who ran it, the sandbox SID and the time (`clauseo_sandbox::windows::marker`).
- **Set-up state.** `SandboxSetup::state` gives `Ready` only for a current marker of this user
  whose SID the record also names; otherwise `Needed`, `Outdated`, `SetUpByOtherUser` or
  `Failed`.
- **Detection.** `detect(setup)` caches only the machine's architecture and build; ARM64 and
  builds before Windows 10 22H2 (19045) are tier C, `UnsupportedOs`. The set-up state is read on
  every call: tier B only when `Ready`.

The plan's decisions (orchestrator, 2026-11-05):

- **D1.** The folder gate runs in the host and again in `SandboxUserLauncher::new`, on every
  folder it grants or denies on; a failing data folder makes the machine tier C.
- **D2.** Claude on a `CantChangePermissions` matter on a tier-B machine is plain tier C, with
  no forced ask.
- **D3.** Codex: try its `unelevated` sandbox nested inside tier B first; if that fails, run
  codex-acp's full-access mode with tier B the only fence, never granting `WRITE_DAC`.
- **D4.** The ACE ledger is write-ahead: `pending`, the ACE, `applied`; pending entries are
  reconciled at start.
- **D5.** The first Windows user owns the set-up; a second user's set-up stops with
  `SetUpFailed` ("set up by another Windows user") and that user stays on tier C.
- **D6.** Detection caches only the OS facts and reads the set-up state at each tier query.
- **D7.** The ACL walk skips only name-surrogate reparse points (junctions, symbolic links);
  cloud placeholders get the ACE through `FILE_FLAG_OPEN_REPARSE_POINT`.
- **D8, replaced.** The per-matter deny-read on matters that `Users` can read is replaced by the
  fences (every other pool user denied each matter) and the group's deny on siblings and loose
  files, above.
- **D9.** One pool user per matter, with the token above.
- **D10.** The browser is off on tier B in v1: Chrome needs `Local\` named objects, which the
  session grants only to the desktop user's logon SID. A launch that needs the browser falls
  back to tier C's rules.
- **D11 (approved; private plan §3 D11, merged #397).** `Everyone` stays among the token's
  restricting SIDs.

### Rejected: AppContainer (spike A)

One AppContainer profile with a capability per matter failed: a container can create pipes only
under `\\.\pipe\LOCAL\`, so Node's spawn hangs and codex-acp never starts codex; Chrome's
crashpad dies; and `NUL` needs an admin-set entry after every boot.

### Accepted risk: `Everyone` in the token

Fix 3 (dropping `Everyone`) failed: bcrypt needs write access to KsecDD, and nsiproxy's IOCTL
`0x120007` admits only `Everyone`, `SYSTEM` or `Administrators`, so name lookups fail. The
mitigation: PR5 sets an explicit DACL on every pipe the runner makes, with a VM test that another
matter's agent gets error 5. The residual risk: a pipe the agent makes with no security set gets
NPFS's default descriptor (`Everyone` read), so another matter's agent can read it. A later tier
may drop `Everyone` behind a proxy that runs as the desktop user.

### Threat model: what stays open, by design

- **FAT and exFAT drives** keep no ACLs, so every pool agent reads them. A matter on one is
  refused by the gate (`NoPermissions`). Owner: S15 close-out.
- **Another matter's allowed, read or runtime paths** beside a matter get no sibling deny, so
  every pool user reads them wherever `Users` can. Owner: S15 close-out, Rohan's read-gap
  decision.
- **A neighbour on a network share** gets no fence: the restricted token fails network logon
  (measured over `\\localhost`). Accepted risk: a share name that aliases a local folder. Owner:
  PR7's VM row (the (B) token against `\\localhost` and a guest share).
- **Folders on `C:\` outside the matter's parent, and `ProgramData`**, stay readable where
  `Users` reads them. Owner: S15 close-out, Rohan's read-gap decision.

## Consequences

- A finished set-up moves the machine to tier B at the next tier query; R17's
  `RetryProtection` needs no restart.
- Each matter's agent runs as its own Windows user. HKCU is read-only to it at Low integrity, and
  its `AppData\LocalLow` persists across matters, so reusing a pool user needs a clean-up that
  empties it and re-owns or removes the old matter's files.
- A second Windows user on the same machine stays on tier C until machine-scope secrets land.
- A lawyer without admin rights can't answer the set-up prompt, so no pool is made and the
  machine stays on tier C. Firms often lock admin rights down, so this is common. A "B-minus"
  tier (the lawyer's own token at Low integrity, no accounts, no prompt) may follow v1.
- Uninstall (S15a, fix 5) removes, for each pool user, the profile and then the account, then the
  entries naming pool SIDs, from the ledger. Deleting the account first leaves its profile folder
  and `ProfileList` key behind.
- The token, the default DACL and the desktop come from spikes on Windows Server 2025; Windows 11
  and Windows 10 22H2 are unmeasured.

## Status

proposed (draft; S15's later pull requests complete it)

## Date

2026-11-05
