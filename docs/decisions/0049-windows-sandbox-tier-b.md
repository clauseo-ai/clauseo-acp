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
  - Order: grants on missing names are left out; the credential link must still be the credentials
    file the launcher names (one file ID, compared again on the handle the write goes through), and
    that file may not sit in the state folder or be the link itself, or the launch is refused, and
    its ledger entry records that file's volume, path and file ID, so forget and rollback open that
    file by its ID (`OpenFileById`), wherever its names now are, never through a link the agent may
    have swapped, and drop the entry when no file has that ID any more;
    every grant is recorded `pending`; a new Low label entry first records the explicit label its
    path has, unless it is our own Low label or another entry's on the same file (the lawyer's own
    label, put back when ours goes); the entries are written; the walks run; only then is every
    entry marked `applied`. An entry still `pending` (a launch, rollback or forget that stopped
    partway) gets its tree walked at the next launch of its matter. `reconcile` walks each pending
    entry's tree at start, plainly: an allow or label entry is then marked or dropped, but a deny
    entry stays `pending`, so its matter's next launch walks it strictly.
  - A tree that carries a deny (the matter's own folders, each sibling, each fence) is walked
    strictly. A child an agent made (a pool user owns it, whatever its number of links) that
    would beat the inherited deny, with an explicit allow for a broad group, another pool user
    or the pool group, or a protected DACL, is repaired: those allows are stripped, the
    protection cleared and its inherited entries worked out again. The repair runs at each launch
    and again when the agent exits (`repair_after_session`). A child the lawyer owns that would
    beat it refuses the launch, naming the child for the running page.
  - A child the lawyer can't open, or can open but not re-permission (no `WRITE_DAC`, or no
    `WRITE_OWNER` for an owner or label change), is set aside through its folder's
    `FILE_DELETE_CHILD` when it sits in this launch's own matter, state or notes tree, or, in a
    fence walk, in another registered matter's tree, into that matter's own set-aside and list,
    so one matter's agent can't block another matter's launch or `fence_registered`; every such
    child is, and the walk runs again after each. Anywhere else (a sibling, parent files), or when
    the set-aside fails (another volume, or no `FILE_DELETE_CHILD`), the launch is refused, naming
    the child, and nothing moves.
  - The matter, state and notes trees are walked at every launch, so a file moved in since picks
    up the inherited entries; other trees when their entry was just written or is still pending.
    Every write keeps a DACL's auto-inherited mark (`SE_DACL_AUTO_INHERIT_REQ`), so a walk with
    nothing changed writes nothing and the lawyer's folders keep their `AI`. As
    `SetNamedSecurityInfo`'s propagation does, a child that isn't protected loses its old inherited
    entries before the strict check and before it is worked out from its parent, and is written
    auto-inherited. On a child that isn't auto-inherited either, old inherited entries include
    explicit ones equal to what the parent alone gives it: under a parent that isn't
    auto-inherited (a plain `C:\m`), NTFS writes its inherited copies without the inherited flag,
    and `icacls` only displays them as `(I)` (VM run 3). The root's write makes it auto-inherited
    too and keeps its inherited entries. Otherwise an old inherited `Users` allow would count as
    explicit and refuse every launch. Paths are compared in one normal form, so a walk's verbatim
    (`\\?\`) paths match the plan's plain ones.
  - Any failure takes back this launch's new entries: under the ledger's lock they are set
    `removing` on disk (a state of its own, so `reconcile` finishes a rollback or forget that a
    crash stopped, rather than mistake it for a launch), then each permission entry this launch
    wrote is removed unless another entry, as read under that lock, names it (a Low label counts as
    named when another entry's label is on the same file); the trees are walked again, and only
    entries whose permission entry is gone are dropped. `forget_matter` takes entries back the same
    way.
  - **A folder that is now a registered matter loses its fix 2 deny once its first launch
    succeeds**, for every matter whose entry names it, and its tree is walked again. A refused
    launch leaves every deny. A deny on a folder that holds the new matter is first replaced, for
    the matter that wrote it, by the gate's walk-down set below that folder: a deny on each
    folder beside the path at every level, and a files-only deny at each level. A matter whose
    replacement failed keeps its own deny.
  - `forget_matter` removes a matter's entries and labels without admin, puts back the lawyer's
    own labels, and hands what the matter's pool user made in its matter, state and notes folders
    back to the desktop user: the agent's explicit labels and broad allows removed and the
    protection cleared first, then the owner set to the desktop user (through the lawyer's
    `WRITE_OWNER`), then the entries worked out again; a file it can't hand back is set aside as
    at launch. The ledger records the hand-back as owed before anything is removed and clears it
    only once every hand-back has succeeded; until then `reconcile` keeps that matter's entries,
    a retried `forget_matter` finishes the job, and `Ledger::handback_owed` tells PR4's reuse
    check not to hand that pool user to another matter. `fence_registered` fences every
    registered matter against a grown pool (PR4 calls it).
  - **Under the `C:\m` layout** (a matter in a folder that inherits `C:\`'s entries), an agent's
    file can shut the lawyer out while the lawyer has no `FILE_DELETE_CHILD` to set it aside, so
    forget's hand-back fails (`PermissionDenied`, naming the file) and stays owed. PR4 must
    provide the elevated clean-up for this: take ownership with admin rights, then hand back.
  - The credential link's allow is read and write of the file's data only (`0x12008f`, no
    `DELETE`); it lands on the lawyer's real credentials file.
  - **Set-aside.** The set-aside scans the matter tree itself, through handles, never following a
    link; the scan writes nothing and has no cap, so nothing the agent does to the ACL walk or the
    repair switches it off. In each folder it probes each protected name (at the root
    `PROTECTED_NAMES` and `.git`; at every depth `.git`, `.claude`, `.mcp.json`, `CLAUDE.md` and
    `AGENTS.md`, one list for the full scan and the fast launch's re-probe) by opening it
    relative to the folder's handle, case-insensitively as the file system decides, so a name counts
    exactly when git or Claude would open it; it records what the probe opened by file ID. It also
    records every junction and symbolic link in the tree by the link's own file ID, whatever its
    name. Names are kept as raw UTF-16, so a name that isn't valid text is recorded like any other.
    Names and paths are compared with `CompareStringOrdinal`, ignoring case, except that what goes
    with a moved item is decided by its exact path or its file ID, so a second entry differing only
    in case (a case-sensitive folder) is still checked. PR6's launcher calls `before_launch` before
    `apply`, and `after_session` when the agent exits, independently of `repair_after_session`'s
    result. Every protected name or link that is new or whose file ID changed is set aside, whoever
    owns it, a link moved as itself and never followed: so a lawyer folder the agent renamed to
    `.git` or `sub\.git`, a lawyer file it hard-linked as `CLAUDE.md`, or a junction it made to a
    folder holding a `.git`, is caught. A scan that can't list a folder still sets aside what it
    found, names the folder in its result, and keeps the session open; names inside a folder it
    couldn't list at launch aren't recorded, so they count as new later. Matters' agents run in
    parallel, and so may two sessions of one matter: the record holds the set of open sessions,
    every `after_session` checks, and `before_launch` checks first whenever a session is open.
    `before_launch` is told which sessions of the matter are still running; every other open session
    crashed, and is dropped once that check has run. PR5 and PR6 call `before_launch` only once a
    crashed session's job object is closed (kill-on-close), so its agent is dead. A protected name a
    live session makes may be set aside when another session of its matter starts, and one the
    lawyer changes during a session is set aside too (both accepted). Items go to
    `<data_dir>\sandbox\set-aside\<matter>\<timestamp>\`, under the item's path below the matter,
    the protected folder the agent can't write, opened part by part by handle without following a
    link and renamed with `NtSetInformationFile`, never over an existing name. A manifest entry is
    written as an intent before each move and finished after it, under the set-aside's lock, and an
    intent a crash left is settled on the next load, so every item set aside is listed (the original
    and new paths, the reason, the owner, the time and the matter). The running page shows them
    (`list_set_aside`), and `restore_set_aside` moves one back only if its name is free. An item on
    another volume than the data folder can't be moved there by a rename: the pass is refused naming
    the item (`OtherVolume`), the item stays, and no copy is made.
  - **Write-denies at any depth (S15 PR4a2).** The policy's walk of the matter (`scan_matter`,
    run by `PolicyBuilder::build` at every launch) lists every `.git`, `.claude`, `.mcp.json`,
    `CLAUDE.md` and `AGENTS.md` at any depth (`PROTECTED_AT_ANY_DEPTH`, the set-aside's nested
    list too), by a name check on the listings it reads anyway, ASCII case ignored as NTFS
    compares these names. `Policy::read_only_in_matter` holds each as a subpath rule, unless a
    folder it lists already holds it, and the plan gives the pool user the same inherited deny
    of writes as the root names (`0xd0156`, `DELETE` and `FILE_DELETE_CHILD` included). With
    `Modify` alone on the matter (no `FILE_DELETE_CHILD`), the agent can't write, delete, rename
    or replace one that exists at launch, and the lawyer's copy stays as it is. The ledger
    records each deny, and forget hands it back like any other. A link with one of these names
    that leads into the matter is listed too, as a `.git` link is, so the permission write
    refuses the launch there, as at the root, with `Error::ProtectedLink` naming the item
    relative to the matter (`matter.protected_link`). macOS reads the same list: its profile
    gains one subpath parameter per item.
  - An owner gets `WRITE_DAC` before any deny is read, so a protected item a pool user owns (one
    the lawyer restored from the set-aside or renamed, or one made before PR4a) is handed to the
    desktop user before its deny is written, as forget hands it back (`AclWriter::hand_back`,
    forget's reclaim settle for the item itself). What a pool user owns below a deny goes the
    same way, by an owner pass (`AclWriter::hand_back_tree`: each owner read through its
    folder's handle, never following a link, only a pool user's item written): over each
    protected tree whose deny the launch writes or finds still pending (a crash between the deny
    and its pass), over every protected tree on a full walk, and on a targeted walk over each
    touched folder inside one (a file the lawyer moved into `.claude\commands` or `.git\hooks`)
    and each protected tree inside a touched tree or directly in a touched folder (a folder
    moved in that holds a `.claude` already denied).
    A skipped launch needs none: nothing changed, and no pool user can add a child below a deny.
    Nothing is recorded for it: the lawyer owning it is where forget leaves it too.
    `restore_set_aside` and the set-aside's moves hand the item's whole tree to the desktop user
    as well, whoever owns its top (a lawyer's folder the agent filled and renamed to `.git`). An
    `OWNER RIGHTS` deny was ruled out: it binds the lawyer too when the lawyer owns the item.
    Accepted: a running session of the same matter could swap an item between its hand-over
    and its deny; the set-aside's file-ID check catches the swapped item.
  - The policy's walk runs at every launch, whatever the launch check (PR3d) decides: the check
    leaves out only the own-tree walks and the set-aside's scan. Every read-only deny is checked
    at every launch and written if it is missing, with its own walk (`apply_checked`). So a
    nested `CLAUDE.md` the lawyer made between sessions gets its deny on a targeted launch. On
    a skipped launch nothing in the trees changed, so the walk lists the same items and each
    deny is found in place. The read-only set is not among the walk inputs (`WalkInputs`) because
    no own-tree walk depends on it, so a changed set can't make a skip miss anything. PR3d's
    measured times leave out the policy's walk; the PR4a2 timing row reports it. If PR6 reuses a
    walk across launches, it must still walk the changed folders again for these names.
- **Records.**
  - `HKLM\SOFTWARE\Clauseo\Sandbox`, with the protected DACL
    `D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)`: `PoolSids` (`REG_MULTI_SZ`), every pool
    user's SID, which the desktop's start-up check (#317) reads through
    `clauseo_sandbox::windows::sandbox_pool_sids`; `GroupSid`, the group's SID; `OwnerSid`, the
    Windows user who owns the set-up (D5; `sandbox_owner_sid`); `DataDir`, the owner's data
    folder; and `Removing` (`REG_DWORD`), set while a removal is unfinished
    (`clauseo_sandbox::windows::pool::MachineRecord`). PR4b ships the readers; PR4e's set-up
    writes the key.
  - The local group `ClauseoSandboxUsers`, holding every pool user (PR4).
  - `<data_dir>\sandbox\matters.json`, replaced whole and atomically and refused, never
    rewritten, when it can't be read (`clauseo_sandbox::windows::matters`): today each matter's
    random `S-1-5-21-<a>-<b>-<c>-<rid>` SID, optional under D9 (`windows::sids`); PR4 adds
    each matter's assigned pool user.
  - `<data_dir>\sandbox\setup.json`, the set-up marker (version 2): the set-up version, the
    Windows user who ran it, the group's SID, the pool's SIDs and the time
    (`clauseo_sandbox::windows::marker`). A version 1 marker reads as `Outdated`.
- **Set-up state.** `SandboxSetup::state` gives `Ready` only for a current marker of this user
  when `OwnerSid` is this user, `GroupSid` is the marker's group and `PoolSids` holds every pool
  SID the marker names; a set `Removing` gives `Failed` ("removal unfinished: reinstall and
  uninstall, or run set-up again"); otherwise `Needed`, `Outdated`, `SetUpByOtherUser` or
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
- **Old inherited entries on a child that isn't protected are dropped**, as Explorer's propagation
  drops them, at every walk that works such a child out, fence walks included: an entry an old tool
  (`cacls`, `xcopy /O`) flagged inherited is lost, isn't recorded, and isn't restored by forget.
  On such a child that isn't auto-inherited, an explicit lawyer entry identical to what the parent
  gives it is taken for an inherited copy and comes back inherited, so access doesn't change; an
  auto-inherited child keeps its explicit entries (accepted).
- **PR4's uninstall hook must delete every reparse point under the app-data folders, without
  following it, before Tauri's NSIS "delete app data" runs `RMDir /r`**, which follows junctions:
  VM run 3 emptied a lawyer folder through a junction set aside under `<data>\sandbox`.
- **The credential-link mechanism stays, but its use for Claude on Windows goes back to PR6's
  design**: Claude Code 2.1.293 rewrites its credentials file by a temp file and a rename, and
  refuses a target with more than one link, so a hard-link credential link goes stale or breaks
  Claude's token refresh (VM run 3).
- **Under the `C:\m` layout the lawyer has no `FILE_DELETE_CHILD`**, so a fence walk can't set
  aside a child the lawyer can't open in another matter's tree, and one matter's agent can still
  block other matters' launches until its own exit repair or next launch; PR4's elevated clean-up
  is the recovery (accepted).
- **A launch killed mid-walk** ends with its own pool user's allow and label entries `applied`
  (`reconcile` finds them present), while its fence and sibling denies stay `pending` (N4), so
  the next launch walks those deny trees strictly and still refuses what it would have refused.
  No agent runs in between, and the allow names only the matter's own pool user on its own matter
  (accepted; no launch marker).
- **A nested protected item one session's agent makes** is visible to a second session of the
  same matter running at the same time, until a set-aside pass runs (at that session's end or
  the next launch). A permission entry can't deny a name that doesn't exist yet, so nothing
  stops the agent making one; what stops it loading in a later session is the set-aside
  (accepted). One that exists at launch can't be edited, renamed or deleted by the agent (S15
  PR4a2, below).
- **A hard-linked file inside a nested `.claude`** (or any denied folder) that the lawyer owns
  stays bare: the walk leaves a file with more than one link alone unless a pool user owns it
  (`acl/walk.rs`), since its DACL belongs to every name. Making a new hard link to a denied
  file needs `FILE_WRITE_ATTRIBUTES`, which the deny takes away (accepted).
- **A deny on an item the lawyer renamed away from a protected name** stays after forget: the
  explicit entry goes with the file, and forget removes entries by path. This predates S15
  PR4a2, for the root names and nested `.git` (accepted).
- **A nested `CLAUDE.local.md` or `AGENTS.override.md`** the agent makes below the matter's
  root is not set aside: the two extra opens per folder cost 6.0% of the scan on average (4.8% to
  6.8% over three runs) on 50,000 files ten to a folder (macOS stand-in), past the 5% allowed for
  them (accepted).
- **The repair undoes a lawyer's own lock-down of an agent's file**: clearing the protection of a
  file a pool user owns puts the inherited entries back, the pool user's `Modify` among them,
  even where the lawyer protected it on purpose; the lawyer's own explicit entries are kept
  (accepted).
- **An item set aside keeps its own permissions**, so an agent they grant could still open it by
  its full path under the data folder. Since S15 PR4a2 an item a pool user owns (the agent's)
  is handed to the desktop user once it is moved and listed, as forget hands back a pool user's
  items, and is worked out again from the set-aside folder, so the entries it inherited in the
  matter go; its manifest entry keeps the owner it had. Only owners in the machine record's
  `PoolSids` count: an item anyone else owns (a colleague's file, or `Administrators'`, which
  the agent can hard-link into an item to be set aside) keeps its owner and permissions, and a
  file with another name is touched only when a pool user owns it. With the record unreadable
  or empty nothing is handed over and that is logged; the item stays listed, and PR4c's reuse
  hold keeps its owner from serving another matter. A lawyer's own item keeps its permissions
  as they were. A hand-back that fails is logged and leaves the item set aside.

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

### Shared writable places

A spike probed 62,145 folders under a Low-integrity sandbox token and found two it could write:
`C:\ProgramData\Microsoft\DeviceSync`, which every pool user shares, and each pool user's own
`AppData\LocalLow`. `DeviceSync` was one of the channels two hijacked agents could use.
`LocalLow` is denied because it is the one Low-writable place outside the session-end repair: a
file an agent makes there with its own DACL would outlive the session.

Each target gets `ClauseoSandboxUsers`' `DenyWrite`, inherited to the tree. The spike measured
`icacls` `(W,DC)`; the code writes `0xd0156`, which adds `DELETE`, `WRITE_DAC` and `WRITE_OWNER`.
Tools running with the full mask are not measured yet: PR4's VM test must confirm them.

`LocalLow` is `SYSTEM`'s, but the files a pool user makes in it are the pool user's, and an owner
always has `WRITE_DAC`. So each `LocalLow` also gets an inherit-only `OWNER RIGHTS` deny
(`OI|CI|IO`, `0x0b`): what is below it loses the owner's implied `WRITE_DAC` and `WRITE_OWNER`, and
`LocalLow` itself keeps `SYSTEM`'s rights. `DeviceSync` is `SYSTEM`'s too, and gets none. The grants
are written, then walked down with the pool group and every pool user watched: a child a pool user
owns is repaired (its explicit allows stripped), as the launch walk does. A child the lawyer owns,
or one the walk can't repair, fails its target. A failed walk keeps the target's deny, which still
blocks new files. Each target rolls back only its own grants, and only when an add fails.

A `LocalLow` exists only after its pool user's first logon, and Windows makes it `SYSTEM`'s, so PR4
logs each pool user on once, with its profile loaded, before it applies these. A missing
`LocalLow` is a failure; a missing `DeviceSync` is skipped.

PR4's elevated set-up applies these after it creates each pool user's profile and logs the user
on, treats any failed target as a set-up failure, and keeps what was written. Uninstall removes
the `LocalLow` denies before it deletes each profile, then the account, then `DeviceSync`'s deny
(`shared_writes.rs`).

### Fast later launches (PR3d)

A later launch spent its time on the strict walk of the matter, state and notes trees and on the
set-aside's scan: 633 ms at 5,000 files and 5.0 s at 50,000, warm, and 12 s cold at 50,000. Both
are deterministic in four inputs: the plan's walk inputs (the own roots, the matter's grants on
them, the strict sets), each root's descriptor and identity, the trees' names and file IDs, and
each child's descriptor. If none changed since a walk that passed, walking again changes nothing
and refuses nothing. The NTFS change journal says whether any did (`clean_close.rs`).

**The record.** At a clean session end PR6 writes `<data_dir>\sandbox\clean\<matter>.bin`:
- the walk inputs as the session-end repair used them (kept whole, not hashed: an exact
  comparison);
- each own root's path, volume serial and 64-bit file ID;
- each volume's journal ID and cursor (`NextUsn`), read before the repair starts, so anything
  the repair or anyone else changes while it runs is replayed at the next launch;
- every name below the roots: its file reference, its folder's, and whether it is a folder,
  read through each folder's handle after the repair, never through a link;
- each volume's first journal record at the cursor (its USN, time and file), or none yet;
- a format version.

The same pass that reads the names writes a close record (`FSCTL_WRITE_USN_CLOSE_RECORD`) for each
root and every item below it, after the repair and the set-aside's pass. NTFS logs a reason once
per open period: while another program holds an item open (Explorer holds the folder it shows),
a second change of a kind already logged gets no record until the last handle closes (Microsoft,
"Change Journal Records"). A close record ends the period, so a change after it is logged afresh.
An item it can't do this for means no record is written. If the VM shows the close record doesn't
reset the logging at the lawyer's token, an edit to an item held open since the session end is a
residual here.

A session end is clean only when the repair passed, `after_session` passed with a complete scan,
no session of the matter is open (checked under the set-aside lock the record is written under),
no ledger entry on, in or above an own root is `pending` or `removing`, no hand-back is owed for
the matter, each root is still the folder the cursor was taken for, and every folder could be
listed and closed. Otherwise no record is written.

**Spent at every launch.** `check_launch` reads the record and removes it under the matter's
set-aside lock before it decides anything. A removal that fails refuses the launch
(`Error::CleanClose`). The set-aside's launch pass removes it again, under the same lock, as it
adds the session, so a record written by a session end racing a launch never stands while that
launch's agent runs.

**The check** (on the host, at the lawyer's Medium token) walks in full when any of these holds:
1. there is no record, it can't be read, it is another matter's, or another version's;
2. a session of the matter is open;
3. the plan's walk inputs differ from the record's;
4. a ledger entry on, in or above an own root is `pending` or `removing`, or a hand-back is owed;
5. a root can't be opened by its path as itself, or has another file ID or volume;
6. a root's drive is removable (`DRIVE_REMOVABLE`, any type but `DRIVE_FIXED`, a USB, 1394, SD
   or MMC bus, or a bus query that fails);
7. the volume isn't NTFS, or its file IDs don't fit 64 bits (ReFS);
8. the volume has no journal, or the query or read fails or isn't supported;
9. the journal ID differs (the journal was deleted and made again);
10. the cursor is below the journal's lowest readable USN (it wrapped), or records past it are
    lost during the read;
11. the cursor is past the journal's next USN (a reset, a restored or rolled-back volume), or the
    first record at the cursor isn't the one the record saw there (another USN, time or file, or
    none now; with none seen then, the first record read must start at the cursor): a volume
    restored to an earlier image that has written past the cursor since;
12. a record touches an own root itself (its descriptor or its name);
13. more changed items than 2,000, or 5% of the trees' names if fewer: past that, walking the
    changed folders one by one costs about what the full walk does, since each changed item's
    folder is listed and its children worked out (about 0.1 ms each) and a folder holds about ten
    names;
14. a changed folder's place can't be read.

Otherwise it keeps the journal records past the cursor whose file is a recorded name or whose
folder is a recorded folder, or one such a record brought in. Data writes, closes, basic
information, extended attributes, object IDs, indexing, streams, compression and integrity can't
change a walk's result and are passed over; every other reason bit counts, an unknown one too.
With none left the launch skips the own-tree walks and the set-aside scan; grants not yet written
are still written and walked. With some, it walks each changed item's folder's children
(`Reach::Children`), and the whole subtree of a changed folder that isn't a link (one moved in
brings its contents with no records of their own), strictly, setting aside and refusing as any
walk does. The set-aside re-probes those folders in the matter and keeps the rest of its record;
a re-probe of a folder's own names also drops what the record holds under a folder in it that is
no longer there, so a name moved back later is new again.
A file whose DACL changed through a hard link outside the trees has its record's folder outside,
so every folder the record holds it in is walked. A folder swapped in for a root has the root's
parent as its parent, outside the trees, which is why each root's file ID is checked by path.

**Why the pool user can't make a launch skip.** Every input is out of its reach:
- the record and the set-aside record live in the sandbox folder, whose protected DACL names only
  the desktop user, `SYSTEM` and `Administrators`, and each is checked before it is read;
- the journal is the kernel's: every change to the volume is logged whoever made it, deleting or
  resizing it needs admin, and a read with a journal ID checks it is the same journal;
- what an agent can do to the journal is fill it so it wraps, which means the full walk;
- the ledger and the plan come from the protected folder and the launcher;
- no agent runs between a clean close and the next launch. **PR6 must call `write_clean_close`
  only after the session's job object (PR5's, kill-on-close) is empty**: the code can't check
  that itself. The record is spent before the next agent starts;
- the root facts come from opening each root by path with `FILE_FLAG_OPEN_REPARSE_POINT`, and the
  drive type and bus from the OS.

**Measured (VM run 5, Windows Server 2025, optimised build, the lawyer at Medium).** A later
launch with nothing changed (the check, the set-aside's launch pass and `apply_checked`) took 20,
26, 22 and 38 ms warm at 1,000, 5,000, 20,000 and 50,000 files, and 49 ms (5,000) and 79 ms
(50,000) cold after a reboot, against targets of 300 ms and 1 s. The session end at 50,000 files
took 8.4 to 12.6 s, of which the close records were 3.2 to 3.5 s; it isn't on the launch path.
The first record past a cursor can start at the next 4 KiB page, since NTFS pads a journal page
rather than split a record; the check and the record accept it there when nothing lies between.

**What the skip doesn't change.** A change made after the check reads the journal and before the
agent starts is not seen until the session-end repair. That window is the one today's walk has
between visiting a folder and the agent's start.

**Accepted risk: changes made by another OS while the volume was offline.** The journal sees only
what Windows mounted. A fixed disk changed under another OS (dual boot, Linux `ntfs-3g`) can carry
a descriptor change no record shows, so the next launch may skip a walk it needed. Removable
drives and volumes on a USB, SD, 1394 or MMC bus always walk in full. Virtual disks (a mounted VHD),
iSCSI disks and Storage Spaces read as fixed disks, so one changed while detached from this Windows
is the same accepted limit. The session-end repair still walks everything at each session end.

**The unprivileged read.** `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` reads the journal through a
folder handle without admin rights, and gives records without names, including those of files the
caller can't open. Microsoft Learn has no page for it. Microsoft's Windows SDK defines it in
`Windows Kits\10\Include\10.0.14393.0\um\winioctl.h` inside the `NTDDI_WIN10_RS1` block
(`#if (_WIN32_WINNT >= _WIN32_WINNT_WIN10_RS1)`), which is Windows 10 1607, build 14393; the
10.0.10240.0 SDK's header lacks it. That is the header's guard, not a statement of support, and the
call was measured only on Windows Server 2025 (build 26100); Windows 11 is a required VM row.
Where the call fails, the launch walks in full.

**For PR4's set-up.** PR4 enlarges the journal on each volume that holds a matter to 256 MB unless
it's already larger (`FSCTL_CREATE_USN_JOURNAL`, `MaximumSize` 256 MB, `AllocationDelta` 32 MB):
capped there, never shrinking one. The default 32 MB held about 400,000 records in S15's
measurements; a wrap only costs a full walk, so a larger journal makes them rarer.

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
