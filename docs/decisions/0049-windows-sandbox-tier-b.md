# 0049: The Windows sandbox, tier B

**Draft.** S15 PR2 starts this record, PR1 brings it to design (B), PR3a and PR3b the ACL walk,
its application and the set-aside.
Later S15 pull requests complete it: set-up and the pool (PR4), the runner and token (PR5), the
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
- **Permission writes (PR3a, `windows/acl.rs`, `windows/acl_bytes.rs`).**
  - Every target is opened by its own name (`FILE_FLAG_OPEN_REPARSE_POINT`, which leaves a cloud
    placeholder unhydrated) and refused if it is a junction or symbolic link now, if its final
    path isn't the planned one, if an allow would reach a file's other hard-linked names (the
    credential link excepted, by design), or for a whole-tree deny, if it is a volume's root.
  - Entries are added and removed exactly, as bytes, in Windows' order, and written with
    `SetKernelObjectSecurity`, which doesn't propagate. Clauseo's walk then works out each child's
    inherited entries from its parent (`CreatePrivateObjectSecurityEx`, as Windows' own
    propagation does) and the inherited Low label, writing only what changed. It skips junctions,
    symbolic links and files with more than one link, which stay bare.
  - The walk looks up no path but its root's, whose final path must be the planned one. Each
    folder is opened once and listed through its handle (`FileIdBothDirectoryInfo`); each child
    is opened by its name relative to that handle (`NtCreateFile` with `RootDirectory` and
    `FILE_OPEN_REPARSE_POINT`), and its final path must be its folder's plus `\name` before it
    is written; a folder's final path is checked again before it is listed. An agent that swaps a
    subfolder for a junction while a walk runs gets the junction opened and skipped, or the walk
    refused (`Changed`) and the launch rolled back; the walk never writes outside the tree.
  - A lawyer's own explicit mandatory label is read before the Low label replaces it
    (`AclWriter::label`), so it can be put back when the Low label goes (`restore_label`).
  - Reconcile's check (`present`) refuses a target that is a link now or has another final path,
    rather than judge the entry by another object.
  - A walk can be strict (PR3b makes every tree that carries a deny strict): a child with a
    protected DACL, or with an explicit allow for a group a pool token holds (`Everyone`,
    `Users`, `Authenticated Users`, `INTERACTIVE`, `RESTRICTED` and the rest of
    `acl_bytes::BROAD_SIDS`), the `None` group of the pool's machine (`S-1-5-21-<machine>-513`;
    the Windows run found it in no pool token, and it stays in the set at no cost), the pool
    group or another pool user, or with an explicit allow of any type but plain or callback (an
    object allow with no object type applies as a plain one), fails the walk: such a child beats
    the inherited deny. Adding explicit denies there instead was not chosen.
- **Applying the plan (PR3b, `windows::apply`).**
  - Order: grants on missing names are left out; the credential link must still be the
    credentials file the launcher names (one file ID), or the launch is refused; every grant is
    recorded `pending`; a new Low label entry first records the explicit label its path has (the
    lawyer's own, put back when ours goes); the entries are written; the walks run; only then is
    every entry marked `applied`. An entry still `pending` (a launch, rollback or forget that
    stopped partway) gets its tree walked at the next launch, and `reconcile` walks each pending
    entry's tree at start before it marks or drops it.
  - A tree that carries a deny (the matter's own folders, each sibling, each fence) is walked
    strictly. A child an agent made (a pool user owns it) that would beat the inherited deny,
    with an explicit allow for a broad group, another pool user or the pool group, or a
    protected DACL, is repaired: those allows are stripped, the protection cleared and its
    inherited entries worked out again. The repair runs at each launch and again when the agent
    exits (`repair_after_session`, which PR6 calls beside the set-aside's `after_session`). A child the lawyer owns that would beat it refuses the
    launch, naming the child for the running page. A child the lawyer can't open at all (a
    DACL an agent made) is set aside through its folder's `FILE_DELETE_CHILD`, or refuses the
    launch, naming it.
  - The matter, state and notes trees are walked at every launch, so a file moved in since picks
    up the inherited entries; other trees when their entry was just written or is still pending.
  - Any failure takes back this launch's new entries: under the ledger's lock they are set
    `pending` on disk, then each permission entry this launch wrote is removed unless another
    entry, as read under that lock, names it; the trees are walked again, and only entries whose
    permission entry is gone are dropped. `forget_matter` takes entries back the same way.
  - **A folder that is now a registered matter loses its fix 2 deny once its first launch
    succeeds**, for every matter whose entry names it, and its tree is walked again. A refused
    launch leaves every deny. A deny on a folder that holds the new matter is first replaced, for
    the matter that wrote it, by the gate's walk-down set below that folder: a deny on each
    folder beside the path at every level, and a files-only deny at each level.
  - `forget_matter` removes a matter's entries and labels without admin, puts back the lawyer's
    own labels, and hands what the matter's pool user made in its matter, state and notes folders
    back to the desktop user: the owner set to the desktop user (through the lawyer's
    `WRITE_OWNER`), the agent's explicit labels and broad allows removed, the entries worked out
    again. `fence_registered` fences every registered matter against a grown pool (PR4 calls
    it).
  - The credential link's allow is read and write of the file's data only (`0x12008f`, no
    `DELETE`); it lands on the lawyer's real credentials file.
  - **Set-aside.** At launch each protected name at the matter's root (`PROTECTED_NAMES` and
    `.git`) is recorded with its file ID (`before_launch`). When the agent exits, PR6's launcher
    calls `after_session`: every protected name that is new or whose file ID changed is set
    aside, whoever owns it, so a lawyer folder the agent renamed to `.git` or a lawyer file it
    hard-linked as `CLAUDE.md` is caught. A session that didn't end cleanly is checked the same
    way at the next launch, before anything else. A protected name the lawyer changes during a
    session is set aside too (accepted). Items go to `<data_dir>\sandbox\set-aside\<matter>\`,
    the protected folder the agent can't write, opened part by part by handle without following a
    link, each with a manifest entry (the original and new paths, the reason, the owner, the time
    and the matter). The running page shows them (`list_set_aside`), and `restore_set_aside`
    moves one back only if its name is free. An item on another volume than the data folder can't
    be moved there by a rename: the set-aside fails (`OtherVolume`), the item stays, and a
    launch whose pre-launch check meets it is refused; no copy is made.
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
- **Hard-linked files and junction targets inside a matter or sibling** stay bare (D7): no fence
  or sibling deny reaches them through the walk, so another pool agent reads a hard-linked file
  wherever `Users` can. Owner: S15 close-out.
- **Between launches**, a file moved into a matter keeps its old entries until the next launch's
  walk. A file moved into a sibling keeps them until something walks that sibling again, which
  launches don't do once its entry is `applied`: a residual until a re-walk. A file moved into
  a neighbour matter's tree picks up its entries at that matter's own launch.
- **Lifting a sibling's deny happens only after the new matter's first launch succeeds.** Until
  then the folder keeps its fix 2 deny, so its new matter's agent reads it only through its own
  entries, which are nearer and come first.
- **A nested `CLAUDE.md`** the agent makes below the matter's root is not set aside.
- **An item set aside keeps its own permissions**, so an agent they grant could still open it by
  its full path under the data folder.

### Accepted risk: two hijacked agents at once

Rohan ruled on 2026-11-05 that tier B runs matters' agents in parallel, with no
one-agent-at-a-time policy and no firm switch for one. The threat model:

- One hijacked matter agent can't read or write another matter.
- Two hijacked agents running at the same time can pass data to each other through channels
  they name: a file the creator gives its own DACL at creation (no `WRITE_DAC` needed, so the
  strict walk repairs it only at the next launch or `after_session`), a pipe with an explicit
  DACL, a FAT or exFAT volume, and `ProgramData\Microsoft\DeviceSync` (a later PR closes
  DeviceSync).
- Dropping `Everyone` from the token wouldn't close the pipe channel against a cooperating
  agent: the creator sets the pipe's DACL itself.

So during a running session an agent's own new files can be opened to another matter's agent,
but only if that agent is also hijacked and running at the same time. The `OWNER RIGHTS` entry
PR3b first planned doesn't close this: the creator passes its own DACL, and `C:\`'s inherited
`CREATOR OWNER` allow gives it full control anyway. Two follow-ups shrink these channels: the
shared-write-place deny PR and the bypass-traverse spike. A third is to weigh a directory
watcher that repairs new pool-owned files during a session.

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
