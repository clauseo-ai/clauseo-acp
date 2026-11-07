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
  - The same holds for a root the plan denies on (S15 PR4h): the matter, state and notes
    folders, a registered matter's, a sibling, the parent. Before anything is recorded, each one a
    pool user owns (a folder an agent made, which the lawyer moved and registered) is handed to
    the desktop user by the same `AclWriter::hand_back`; the state folder's read-only settings
    get the protected items' hand-back too. Only a root whose hand-back fails (the lawyer's
    handle got no `WRITE_OWNER` or `WRITE_DAC`) refuses the launch, as
    `PoolOwned { owned: Root }`, naming it; set-up's fence of the registered matters skips such a
    matter, reports it (`SetupReport::fence_skipped`) and fences the rest.
  - In a strict walk of a matter's own tree, or of a registered matter's fenced tree, a file or
    folder at any depth that a pool user other than that matter's own owns (one the lawyer moved
    in from another matter, say) keeps that user's `WRITE_DAC`, and that user reaches it by its
    path. It is handed to the desktop user and cleaned as forget's reclaim does; its owner comes
    from the descriptor the walk reads anyway. Only a child whose hand-back fails refuses the
    launch, as `PoolOwned { owned: Child }`, naming it, and the launch's entries roll back. That
    is the fail-closed choice: such a child isn't set aside, since it opens fine and only its
    owner can't be changed, and moving another matter's agent's item out of sight would hide
    the problem. The matter's own pool user owning its files is expected, and a child the lawyer
    can't open takes the set-aside path below. PR4i's elevated clean-up takes ownership of the
    roots and children the lawyer's own hand-back can't ("The elevated clean-up", below).
  - Every hand-back (these, the protected items', the owner pass, forget's) writes the owner
    first, then the cleaned DACL, through the one no-follow handle, and reads the item again
    through it: an owner other than the desktop user, or an explicit allow left for a pool SID
    other than the matter's own, refuses (`PoolOwned`). Once the desktop user owns it, the pool
    user has no owner's `WRITE_DAC` to put an allow back with. A file with more than one link is
    never re-owned through one name (`LinkedFile`). A root handed back gets the entries the
    ledger lists on it written again at once, the matter's own `Modify` included, which the
    cleaning strips. Residual: a handle the pool user opened with `WRITE_DAC` before the owner
    write keeps that right, and Windows can't revoke it, so it can still add an allow after.
    The next strict walk refuses that child as `BroadAllowChild`; PR4i and the per-session
    processes bound how long such a handle lives.
  - Residual: a launch that hands back another registered matter's root (its fence) while that
    matter's agent runs strips that matter's own `Modify` until the entries are written again, a
    moment later. A write that fails is tried once more; failing again, its ledger entry is left
    `pending`, logged once without a path, and the matter's next launch writes it. That matter's
    agent may so lose write access to its own matter for the rest of its session: fail closed,
    never open.
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
  PR7's VM rows (`s15_wfp_vm`: the (B) token against `\\localhost\C$` and a guest share).
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
- **The per-machine uninstaller's copy of itself in the user's `%TEMP%`**, started elevated
  before any script can act ("Admin copy", "Closed: the per-machine uninstaller's copy"): closed
  for installs registered by this version, which run it in place (`_?=`) with its plug-ins in the
  administrators-only folder. An install registered by an older version keeps its old
  `UninstallString`, and so the copy, until it's reinstalled.

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

### Accepted risk: the setup exe in a per-user install folder

The setup exe (`clauseo-sandbox-setup.exe`, S15 PR4h) runs elevated from the install folder. In a
per-user install that folder is the lawyer's own, so a standard user's process can write it.
Malware running as that user can swap the exe, and when an administrator then types their
password at the prompt (over-the-shoulder elevation) the malware runs as administrator. Its
publisher shows as unknown either way, since Windows ships unsigned for now.

A DLL planted beside the exe is mitigated, pending the VM re-run. Direct imports are covered by
`DependentLoadFlags`: the link flag `/DEPENDENTLOADFLAG:0x800` makes the loader take the exe's
static imports from System32 only, and the VM showed every direct import (`netapi32`, `userenv`,
`crypt32`, `psapi`, `bcryptprimitives`, `ntdll`, `VCRUNTIME140`) loading from there. The flag
doesn't reach those DLLs' own imports, which the loader resolves before `main` with the exe's
folder searched first. `netapi32.dll` imports `netutils.dll` and `samcli.dll`, which aren't
KnownDLLs, and on the VM a `netutils.dll` planted beside the exe was mapped executable. Transitive
imports were not mitigated before this change: in a user-writable install folder, a planted DLL ran
elevated.

Transitive imports are now covered by delay-load plus `SetDefaultDllDirectories`. `netapi32.dll`,
`userenv.dll` and `wtsapi32.dll` (the stop's process list, `WTSEnumerateProcessesExW`) are linked
`/DELAYLOAD` (with `delayimp.lib`), so they load at their first call.
That comes after the first statement of `main`,
`SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)`, so the delay-load helper's load and the
loads of those DLLs' own imports search System32 only. No code runs before `main` that could call
them. A CI test reads the exe's import and delay-import directories: those DLLs are delay-imports
only, and every remaining normal import is a KnownDLL, an API set, or one of a vetted few whose own
imports are all KnownDLLs; a new import fails the test by name. The VM re-run plants a DLL named
after each import of `netapi32.dll` and `userenv.dll` beside the exe, runs `--set-up` (and, for
`wtsapi32.dll`'s imports, `--stop`), and checks
with Process Monitor that nothing loads from the exe's folder. If the delay-load proves unworkable,
those calls move to `LoadLibraryExW(..., LOAD_LIBRARY_SEARCH_SYSTEM32)` and `GetProcAddress`.

A planted DLL stays the same class of risk as swapping the exe: both need an install folder a
standard user can write. The admin copy, below, closes both for per-user installs too: the exe
that runs elevated is never the one in the install folder.

### Admin copy

Clauseo starts the setup exe elevated from one place only: `%ProgramFiles%\Clauseo\Sandbox\`
(`FOLDERID_ProgramFiles`, never an environment variable), where the installer's elevated step puts
a copy, for per-user and per-machine installs alike. The copy beside `Clauseo.exe` is never
started elevated, by the app or by the uninstall hook.

- **Placing it.** The installer extracts the exe there and runs that copy with `--adopt
  --user-install-dir <path>` plus, per user, `--owner-sid <sid> --data-dir <path>` (both from the
  unelevated installer, the lawyer's own), or `--per-machine`. `--adopt` refuses unless it is
  elevated (or SYSTEM, per machine) and `%ProgramFiles%`, `Clauseo`, `Sandbox` and the copy are
  no links and its own image's final path (`GetFinalPathNameByHandleW`) is the admin copy's (exit
  8). Per user, the lawyer is found one hop past D-B2's parent: `--adopt`'s parent is the
  elevated installer (an administrator's account, over the shoulder), whose recorded parent is
  the unelevated installer. Both must run the same image, be created in order, the outer one's
  token not elevated and its user `--owner-sid`; the elevated account's user and profile are
  never used. That checks the tree's shape, not that it is the installer's: any two processes of
  one image pass, the middle one elevated or not. Nothing is gained by it, since the owner is the
  outer process's own unelevated user, who named themselves. The install and data folders must
  lie inside that SID's `ProfileList` profile.
  Per machine, the install folder may be anywhere, but only SYSTEM, Administrators or
  TrustedInstaller may change it.
- **One owner, of one kind.** `AdminCopyOwner` records whose install placed the copy: the
  lawyer's SID, or `machine`. A per-user `--adopt` refuses a pool owned by another SID
  (`OwnerSid`) or a copy another install placed; a per-machine one refuses a copy a per-user
  install placed. Either exits 2 (`Blocked::OtherAccount`), changing nothing; a second user's
  launch and set-up meet the same cause. `AdminCopyVersion` (the app's version) newer than the
  running copy's own exits 9: no downgrade (`admin_copy::version_not_older`, which the refresh
  task uses too). Then `--adopt` deletes everything in `Sandbox` but the copy, its `.sig` and
  `staging\`, gives the folder and everything left owner Administrators and a protected DACL
  (SYSTEM and Administrators full, Users read and run), reads each back exactly, records
  `AdminCopySha256`, `UserInstallDir`, `AdminCopyVersion` and `AdminCopyOwner`, and, per
  user, runs the normal set-up in-process. Exits: 0 done, 1 usage, 2 another account, 3 failed,
  5 lock held, 6 data folder not the user's, 7 not elevated, 8 not the admin copy, 9 downgrade.
- **`UserInstallDir` is untrusted.** It is checked as text and its last folder for a link, but
  the lawyer can re-point any folder on the way through a junction at any time. Anything SYSTEM
  does with it (the refresh task reading a new exe from it) must open every component without
  following reparse points, and verify what it read before use.
- **Before each start, and each tier-B launch** (`UserSetup`, `check_launch`), a pool another
  user owns is `Blocked::OtherAccount`, and owners that can't be read are `AdminCopyMissing`
  (never "no other owner"). Then the copy must be a regular file (else `AdminCopyMissing`); each
  of `%ProgramFiles%`, `Clauseo`, `Sandbox` and the copy, and each folder between the volume
  root and `%ProgramFiles%`, must be no reparse point, owned by SYSTEM, Administrators or
  TrustedInstaller, with no allow entry that applies to it giving anyone else write, append,
  write-attributes, delete, delete-child, `WRITE_DAC`, `WRITE_OWNER` or a generic right holding
  one; the volume root the same, but only delete-child, `WRITE_DAC`, `WRITE_OWNER` and the
  generic rights that hold them count there, since every Windows install lets Authenticated Users
  add a folder at the root; `Sandbox` must hold nothing but the copy, its `.sig` and `staging\`
  (else `AdminCopyUnsafe`; deny entries are ignored); and its SHA-256 must be the recorded one
  (else `AdminCopyChanged`). Each is `sandbox.blocked`, not retryable, with words for IT. A
  tier-B launch hashes the copy each time.
- **The elevated start holds what it hashed.** `UserSetup` hashes the copy through a handle that
  shares read only (no write, no delete) and keeps it open until `ShellExecuteExW` has returned
  (`admin_copy::launch_checked`), so the file started is the file hashed. The start looks its
  verb up in `HKLM\SOFTWARE\Classes\exefile` alone, passed as the class key
  (`SEE_MASK_CLASSKEY`), never in `HKCU\Software\Classes`, which the lawyer's processes can
  write.
- **Windows security blocking the copy is `ToolMissing`.** When the elevated start fails with
  `ERROR_VIRUS_INFECTED` (225) or `ERROR_VIRUS_DELETED` (226), Windows security (Defender, which
  has flagged the set-up program as a false positive) refused the copy and may have deleted it:
  `Blocked::ToolMissing` (`tool_missing`), `sandbox.blocked`. Only a Win32 `HRESULT`
  (`0x8007xxxx`) or a plain Win32 code is read for its code (`launch_code`); any other
  `HRESULT` is a failure naming it, never a small code such as 226. Every other start failure
  maps as before. A copy missing before the start stays `AdminCopyMissing`, except that the
  start also records `SecurityBlockedAt` (a `REG_QWORD` `FILETIME` in
  `HKCU\Software\Clauseo\Sandbox`): while it is there, a copy that isn't there as a regular file
  is `ToolMissing` in the launch check, the start and the state, since after a 226 the copy is
  gone and "get the installer" would only have Windows security remove the reinstalled copy
  again. A start that succeeds, or a copy found and verified (hash the recorded one), deletes the
  value. It is per user and trusted for nothing but which message to show: it can only turn
  `AdminCopyMissing` into `ToolMissing`, and both refuse. Whoever wires `check_launch` or
  `UserSetup` (R17 PR-E, PR6) treats `Error::Blocked`, `ToolMissing` included, as a refusal,
  never a fallback to a weaker tier, and tests that. The check of the exes beside the app this
  cause first had (`bundled`, before design B) is gone: nothing starts the copy beside the app.
- **The state shows the repair first.** `UserSetup::state` checks the copy before the marker
  and record: missing (or no recorded hash), changed or unsafe is `SetupState::Blocked` with that
  cause, at the first open after a declined or silent install; `Needed` only with a verified
  copy. The state keeps the copy's hash for its file identity (volume, file ID, size, last write)
  between calls; the launch and the elevated start hash it every time.
- **Two removals.** The app's in-app removal runs plain `--remove`: the pool, its entries and the
  rest go, and the set-up's record values with them (`OwnerSid`, `GroupSid`, `PoolSids`,
  `DataDir`, `Removing`, `ResetSids`, `StopCompleted`, `StopSequence`, `StopNext`), but the copy, its four values and the SYSTEM tasks
  (refresh and stop) stay, so a later
  in-app set-up elevates the verified copy with no installer. `AdminCopyOwner` staying keeps the
  one-owner rule: the same lawyer sets up again, another user is `OtherAccount`. The
  uninstaller's hook runs `--remove --uninstall`: all that, then, once the pool has no owner and
  only for the install that placed the copy (a per-machine removal for `machine`, the lawyer's
  own for theirs), the four values, the refresh and stop tasks (`unregister_task`), staging
  (`remove_staging`), everything in the folder, then the folder; the key goes once empty. The
  running copy is renamed in place to a name nothing starts and scheduled for deletion at
  restart (`MOVEFILE_DELAY_UNTIL_REBOOT`), with the folder after it; a reinstall before the
  restart keeps its new copy.
- **The copy beside the app stays.** A per-user install still ships
  `clauseo-sandbox-setup.exe` beside `Clauseo.exe`, with its `requireAdministrator` manifest: it
  is the installer's payload and the refresh task's source, and nothing starts it.
- **What stays open.** The installer's elevated step runs from wherever the lawyer started the
  installer; a swapped installer is outside this decision. While it runs, the unelevated
  installer holds its own exe open sharing only reads, which keeps other Windows accounts from
  rewriting, renaming or deleting it; a process already running as the lawyer can close that
  handle in the installer (`DuplicateHandle` with close-source) or inject into it, then swap the
  file. Nor is the window closed when the elevated step opens the exe: same-user code may still
  change the installer's data after UAC consent (say, through a writable mapping of the file made
  before that open), so the prompt shows the real publisher while what runs is changed. This is
  part of the accepted same-user residual, not closed; a Windows VM test of the mapped view would
  settle it.
- **The running checks are correctness guards, not security boundaries.** The installer refuses
  the elevated step while Clauseo's main window (class `ClauseoMainWindow`) is on the desktop,
  twice: the outer instance looks once, before a UAC prompt that can wait indefinitely, and the
  elevated instance looks (`FindWindow`) after the exe is already open. They keep a running
  Clauseo from racing its own set-up; they don't keep out the residual above. Clauseo's jobs are
  kill-on-close, but that doesn't bound it: a tier-C agent can start code outside those jobs in
  one step (`schtasks`, an HKCU `Run` key, WMI `Win32_Process.Create`), so any same-user code,
  a tier-C agent's included, can race. A same-user process can also fake that window and so block
  the elevated step: a denial of service only, accepted.
- **Plug-ins load from an administrators-only folder.** NSIS extracts every plug-in DLL into
  `$PLUGINSDIR`, which its first plug-in call makes inside `$TEMP`: for an elevated split-token
  administrator, the user's own `%TEMP%` (HKCU `Environment` picks it). NSIS locks that plug-in
  folder itself, but the user's code can rename it inside its writable parent and put its own
  folder and DLL at the name before the elevated load. So the first thing every elevated path does
  (`safe_temp.nsh`, `CLAUSEO_SAFE_PLUGINSDIR`): the per-machine installer at the top of `.onInit`
  (template block `admin-section`, before `SetContext`'s `${RunningX64}`, the language picker and
  `SemverCompare`), and the per-user elevated step once it has its switch, though it loads no
  plug-in. It makes a folder named by `GetTempFileName` under
  `%ProgramFiles%\Clauseo\installer-temp` (`$PROGRAMFILES64`, from HKLM), sets owner
  Administrators and a protected DACL (Administrators and SYSTEM full) with `icacls`, checking
  both exit codes, then sets `$PLUGINSDIR` and `$TEMP` to it. NSIS skips its own plug-in folder
  when `$PLUGINSDIR` is set (makensis `build.cpp:3806-3807`) and deletes `$PLUGINSDIR`, and only
  it, at exit (`Main.c:470-489`). The per-machine installer also points the environment's `TEMP`
  and `TMP` there, so what it starts elevated (the WebView2 bootstrapper, `--adopt`, the old
  uninstaller its reinstall page runs in place) uses it too. The folder can't be checked as ours
  without a plug-in; it is ours because only administrators can create anything under Program
  Files, so no one else could have made it or a link on its path first. Any failure stops before
  anything loads: the per-machine installer exits 25 (a box unless `/S`), the per-user step 99. It
  runs on every per-machine run, SYSTEM's (Intune) included: plain NSIS can't read its token, and
  the environment it could read (`USERNAME`, `TEMP`) is the user's to set, so it can't tell SYSTEM
  from a lawyer, and for SYSTEM the folder is harmless. `$WINDIR\SystemTemp` would serve SYSTEM
  but isn't on every supported Windows, so Program Files is used everywhere.
- **Also open: the NSIS stub's own `%TEMP%` work.** Before `.onInit`, so before any script
  runs, the stub creates the `%TEMP%` folder, which comes from the user's environment (HKCU
  `Environment`'s `TEMP` picks the path), makes one `nsX????.tmp` in it and deletes it (NSIS 3.11
  `Main.c:71-80`, `:259-284`). Elevated, that gives an admin-owned empty folder at a path the
  user's code chose, and a one-shot race on the delete. No way to run code through it is known;
  the per-machine installer had the same exposure, and the script can't prevent it.
- **Closed: the per-machine uninstaller's copy of itself in `%TEMP%`.** Without `_?=` on its
  command line, the elevated stub makes `%TEMP%\~nsu<n>.tmp`, copies itself there as `Un.exe` and
  starts that copy elevated before any script runs (NSIS 3.11 `Main.c:322-420`); the user's code
  could rename the folder between the copy and the start. The per-machine installer now registers
  `UninstallString` `"<folder>\uninstall.exe" _?=<folder>` and `QuietUninstallString` with `/S`
  before the `_?=` (template block `uninstall-string`), so the uninstaller runs in place. Its
  `un.onInit` first makes the administrators-only plug-in folder (block `un-admin-section`, the
  installer's `CLAUSEO_MACHINE_START`, exit 25 on failure, its box skipped under `/S`, `/P` and
  `/UPDATE`), so `SetContext`'s System and `nsis_tauri_utils` no longer load from `%TEMP%`. It
  then refuses (exit 26) unless `$EXEDIR` is `$INSTDIR`: `_?=` takes the rest of the line as the
  folder, so an option appended after it would otherwise unregister Clauseo and leave its files,
  and a start without `_?=` (the stub's `%TEMP%` copy) is refused too. NSIS holds its own exe
  open without delete sharing until exit (`fileform.c:192`, `util.c:465`, `Main.c:472`), so at
  the end of the section (`uninstall_in_place.nsh`, block `uninstall-self-delete`) it starts,
  once (from `un.onGUIEnd` when it has a window, from the section under `/S`; in `$SYSDIR`), a
  hidden `"$SYSDIR\cmd.exe" /d /v:off /c` that waits about 3 s on `$SYSDIR\PING.EXE`, deletes
  `uninstall.exe` only if the main exe isn't there, and removes `installer-temp` and the install
  folder only if empty. An install writes the main exe before `WriteUninstaller`, so the guard
  keeps a new install's `uninstall.exe` unless both land in the instant between `if` and `del`.
  `/d` keeps out HKCU's `AutoRun`, which an elevated cmd would otherwise run from the user's
  hive. It builds no line, and leaves the file, when the paths hold one of `% ^ & | < > " !`.
  The reinstall page runs the old uninstaller this way too, never with `/UPDATE`; its own
  `_?=` comes last on the line, which NSIS takes, and the self-delete can only remove the old
  `uninstall.exe` and empty folders, so it isn't skipped there. **Residual:** an install
  registered by an older version keeps its old `UninstallString` (no `_?=`), and so the `%TEMP%`
  copy, until it's reinstalled. `uninstall.exe` started by hand without `_?=` still copies itself
  before any script runs; the copy then refuses with 26, but by then it is already running
  elevated. Not yet run on Windows.
- Keeping the copy current after an app update is the refresh task's job, a later change.

### Removal (`--remove`, PR4j)

The uninstaller's hook runs `clauseo-sandbox-setup.exe --remove` elevated (`runas`), the admin
copy only, once it passes the app's checks done in NSIS: every level from the drive's root to the
copy admin-only (the rules above), then the copy's SHA-256, computed in the uninstaller's own
process (bcrypt through `System::Call`, the file opened sharing read only; no `certutil`, which
firms' AppLocker, WDAC or ASR rules may block), equal, ignoring case, to `AdminCopySha256`, read
once more on a mismatch in case a refresh landed in between. Otherwise, or when anything can't be
read or computed, it starts nothing, keeps `sandbox\` and the record, and under `/S` exits 27. It
says the hidden accounts are still there (a box unless `/S`) only when the record names an owner,
per user the uninstalling user; otherwise a details line only. Like the app, it looks the verb up
in `HKLM\SOFTWARE\Classes\exefile` alone. The
per-machine installer, already elevated, opens it with `open` and adds `--per-machine`, chosen
when it is built (the template's `INSTALLMODE`; only `currentUser` and `perMachine` build,
stage F), never at run time. `--per-machine` runs only in an elevated or SYSTEM process. The exe
takes the user who asked from its recorded parent (D-B2), never from an argument.

- **Who removes (D-B9 as ruled).** Per-user, only the record's owner. Anyone else's uninstall
  exits 0 having touched nothing; a parent that can't be found exits 3, nothing touched.
  Per-machine, every pool in the record goes, whoever uninstalls, LOCAL SYSTEM under Intune's
  silent `/S` included.
- **A silent per-user uninstall** (`/S`, stage F on #619) never raises UAC: the hook skips the
  removal and goes on as after a declined prompt (1223), so `sandbox\` and the record are kept,
  with one line in the details list (`removeSkippedSilent`) and no box. The set-up is removed by
  reinstalling and uninstalling interactively, or before the silent uninstall from an elevated
  prompt with `--remove --per-machine` (an elevated process may remove the whole record, whoever
  asks; when the owner asks, the ledger work still runs as the owner).
- **Order**, under the set-up lock, waited for at most 60 s (else exit 5, nothing touched; with
  no record, the lock is still tried without waiting, and a set-up holding it is exit 5):
  `Removing = 1`; every process running as a pool SID stopped (D-B13); per pool user, its
  `LocalLow` denies (planned again from the record, D-B11), its profile, then its account and
  sign-in screen value; `DeviceSync`'s deny; the ledger's entries; the group; the record. The
  first failure stops the run: exit 4, the record kept with `Removing` and every pool SID (so a
  later run still trusts the ledger), the ledger kept. A re-run skips what is gone.
- **The ledger, as the owner** (the parent is the owner): `DataDir`, which the owner can
  influence, is checked as `--data-dir` is and used only while acting as the owner; every matter
  is forgotten (`forget_matter`) acting as the owner, with a standard user's token: an elevated
  parent's (the per-machine uninstaller's) is copied with Administrators deny-only, no privilege
  but `SeChangeNotifyPrivilege` and Medium integrity first. If that fails, a per-machine run takes
  the administrator's path below; a per-user run exits 3, nothing touched. Anything left keeps
  the record.
- **The ledger, as an administrator for an absent owner** (per-machine; no impersonation): the
  ledger is read from a `DataDir` the owner owns, opened relative to its checked `sandbox`
  folder's handle, and never written. The only change is removing one access entry, for an entry
  that names a recorded pool SID or the group, is no label, records no file by ID, lies in a
  matter folder the ledger records (not a drive's root), and whose folder, checked right before,
  has no junction or link on the way, is the owner's, and passes the gate's folder check; and
  taking the recorded Low label off each such matter folder's root (removing a label only
  tightens). So a crafted ledger can't steer an administrator onto anything the owner couldn't
  change themselves (K1). No walk, label written, hand-back, add or delete.
- **Exits.** 0 (`SetupExit::Removed`) is the only one the hook reads as removed; only then may
  the app delete `sandbox\` when the box is ticked. The hook reads it only after an ended wait
  (`WAIT_OBJECT_0`) and a successful `GetExitCodeProcess`; a timeout, `WAIT_FAILED` or a failed
  read counts as a failed removal. The hook waits 120 s at most and never ends
  the exe: each step leaves a state the next run starts from. After a timeout with the box
  ticked, the lawyer sees the usual kept-folder box for `sandbox\` while the tool may still be
  writing there; that is accepted, since the tool keeps its records consistent at every step and
  reinstalling then uninstalling recovers.
- **Recovery** from a partial removal: reinstall Clauseo, then uninstall it again.

**Accepted residual.** A per-machine removal someone other than the owner runs deletes the record
even when the ledger lists entries it may not touch, as ruled. Those entries name dead SIDs, which
hold nothing (K10). The Low label comes off each matter folder's root, but its inherited copies
below the root stay (no walk runs for an absent owner), as do labels on folders that fail the
check: there a Low-integrity process of the owner's may still write where the owner can. The
owner's own uninstall first, or the per-user install, avoids it.

### Shared writable places

A spike probed 62,145 folders under a Low-integrity sandbox token and found two it could write:
`C:\ProgramData\Microsoft\DeviceSync`, which every pool user shares, and each pool user's own
`AppData\LocalLow`. `DeviceSync` was one of the channels two hijacked agents could use.
`LocalLow` is denied because it is the one Low-writable place outside the session-end repair: a
file an agent makes there with its own DACL would outlive the session.

Each target gets `ClauseoSandboxUsers`' `DenyWrite`, inherited to the tree. The spike measured
`icacls` `(W,DC)`; the code writes `0xd0156`, which adds `DELETE`, `WRITE_DAC` and `WRITE_OWNER`.
Tools running with the full mask are not measured yet: PR4's VM test must confirm them.

No pool user owns its `LocalLow`: set-up makes sure of it. A fresh profile's `LocalLow` can be
owned by whichever account loaded the profile (PR4f's first elevated CI run found one that wasn't
`SYSTEM`), and an owner keeps an implied `WRITE_DAC` on the folder itself, which would let a pool
user that owned it drop the group's deny. So before any deny is written, set-up reads each
`LocalLow`'s owner and hands one that isn't `SYSTEM`, `Administrators` or the set-up's own account
to `Administrators` (owner only, the folder alone, never through a link), reads it again, and fails
(`Step::Owners`) if it is still anyone else's. The files a pool user makes in `LocalLow` are the
pool user's, so each `LocalLow` also gets an inherit-only `OWNER RIGHTS` deny (`OI|CI|IO`,
`0x0b`): what is below it loses the owner's implied `WRITE_DAC` and `WRITE_OWNER`, and `LocalLow`
itself keeps its non-pool owner's rights. `DeviceSync` is Windows' (`SYSTEM` or `TrustedInstaller`)
and gets none; set-up only checks that no pool user or the group owns it, and fails if one does.
The grants are written, then walked down with the pool group and every pool user watched: a child
a pool user owns is repaired (its explicit allows stripped), as the launch walk does. A child the
lawyer owns, or one the walk can't repair, fails its target. A failed walk keeps the target's
deny, which still blocks new files. Each target rolls back only its own grants, and only when an
add fails.

A `LocalLow` exists only after its pool user's first logon, so PR4 logs each pool user on once, with
its profile loaded, and settles its owner as above, before it applies these. A missing
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

### The elevated clean-up (PR4i)

A launch refused as `PoolOwned` (root or child) or as `ChildShut` it couldn't set aside records
its item in the ledger. So does a forget whose hand-back stays owed. `UserSetup::reclaim` then
runs the setup exe's `--reclaim` behind one prompt. It takes the owner from its verified parent
(D-B2) and reads the data folder from the machine's record. It reads the ledger and
`matters.json` as the owner. On each object a pool SID of the record owns, where the owner's own
token lacks `READ_CONTROL`, `WRITE_DAC` or `WRITE_OWNER`, it sets the owner to the lawyer and
writes nothing else.

The owner writes those files, so they bound nothing. Access bounds the walk instead (K1):

- A root must lie in a matter root the ledger records, both resolved by handle.
- A root must not be a volume's root, nor hold or lie in a pool user's profile.
- A root must be a pool user's, or one the owner can list.
- The walk enters only folders a pool user owns or the owner can list, so
  `SeBackupPrivilege` never opens what neither could.

The app then runs a narrow hand-back as the lawyer, on those trees only. It takes off only the
explicit allows of pool users other than the matter's own. It keeps every other entry, the
protection, and the allows the ledger lists as ours. An item left protected with no explicit
entry, which only the agent's protection can have made, is unprotected and takes the matter's
inherited entries, fence included. After that, the app retries the forget or the launch.

**Accepted: a `PoolOwned` root outside every recorded matter root needs a manual
take-ownership.** Examples are a new matter's own folder at its first launch, a sibling, or the
parent. The clean-up refuses such a root, since the ledger can't vouch for it. The remedy the
host and app give is: forget the matter and add it again; if the lawyer can't take it back, take
ownership by hand as an administrator.

**Set-aside restore stays unelevated.** It must raise no prompt. A restored item whose hand-over
fails is refused, named, at the next launch's hand-back before its deny. That refusal records it
for `--reclaim`.

**Reparse points: elevated code never resolves a path through a reparse point it didn't
check.** A pool user can make junctions, and with the right, symbolic links, in its own profile
and in the matter it works in; a mount point is a junction. So every elevated open below a
profile or a matter is either relative to a checked handle, one part at a time
(`OBJ_DONT_REPARSE`, each part refused if it is a reparse point), or by path with the final path
checked against the planned one (`same_path`). A pool user's `AppData\LocalLow` is reached from
the profile's handle, the profile itself checked against `ProfileList` (`acl/profile.rs`); a
reparse point on the way stops the set-up at `Owners`, naming the part, and `--remove` lets that
user's `LocalLow` deny go with the profile. Before `DeleteProfileW`, every link in the profile,
and every extra name of a file, is deleted itself on its own handle, so the profile service's
delete meets none. A folder's DACL is written with `SetKernelObjectSecurity`, which never
propagates to what is already inside, a planted junction included (`private_dir.rs`); the
permission walk writes each child on its own handle and skips links. Guidance never sends an
administrator to a recursive tool that follows links (`takeown /R`, `icacls /T`), nor to
`rd /s /q`, which can clear read-only on a hard-linked file's other names: a left-over profile
folder points to firm-it's "Clearing a leftover Windows sandbox by hand". These opens enable the backup and restore privileges, with
backup intent, so a folder a pool user shut to administrators can't stop a set-up or a removal.
They fail closed: a profiles folder reached through a junction (a redirected `C:\Users`), or a
`ProfileList` path in 8.3 form, isn't the final path, so set-up stops at `Owners` and a removal
leaves that profile, both naming the part.

### The runner (PR5)

`clauseo-sandbox-run.exe` starts each agent; `windows/runner.rs` holds the chain.

- **Two runners.** The host starts the outer runner as the lawyer through `spawn_in_tree`, so its
  pipes, job and shutdown ladder are the ones every agent has. The outer reads a launch file from
  `<data_dir>\sandbox\run\` only if the lawyer owns it and only the lawyer, `SYSTEM` and
  `Administrators` may open it, and removes it. It reads the pool user's password from
  `secrets.bin` into a buffer wiped as soon as `CreateProcessWithLogonW` (`LOGON_WITH_PROFILE`)
  returns, and starts itself again, suspended, as the pool user (the inner runner). The inner
  goes into a job with `KILL_ON_JOB_CLOSE` and no breakaway before it runs; the job's one handle
  is the outer's, so ending the outer (the host's job does) ends the inner runner and the agent's
  whole tree. Error 1058 (Secondary Logon disabled) is `SetUpFailed` with set-up's own cause; a
  secret that couldn't be read fails that launch only.
- **DLL loads.** Both runners call `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)` first
  in `main`, and the exe is linked as the setup exe is (`build.rs`): its static imports load
  from System32 (`/DEPENDENTLOADFLAG:0x800`), and `userenv.dll`, whose own imports aren't all
  KnownDLLs, is delay-loaded; a CI test reads the import directories. The inner runner starts in
  System32, a folder its Low agent can't write. An agent exit code in the runner's own range
  comes back as `AgentReserved`, never as a runner or set-up failure. The host removes the
  launch file if the outer exits without having removed it. If the outer is killed between the
  logon and the job, a suspended inner runner is left that never runs.
- **A window station and desktop of the agent's own.** The outer makes a window station,
  `ClauseoRun-<16 hex>`, holding one desktop, each naming only `SYSTEM` and the pool user,
  labelled Low, before the inner runner joins the job; the agent starts on it. Neither the
  lawyer's `WinSta0` (whose DACL names the logon SID, deny-only in the token) nor a service
  window station grants the token anything, and user32 then fails to start the agent
  (`0xC0000142`; the first Windows runs of the harness). Its own station also keeps the agent's
  clipboard and global atoms apart from the lawyer's.
- **UI limits on the job:** reading and writing the clipboard, global atoms, other processes'
  USER handles, system parameters, display settings, exiting Windows, and making or switching
  desktops. `WinSta0`, not the desktop, owns the clipboard and the global atom table, so a
  private desktop alone doesn't keep an agent from the lawyer's; Chromium's sandbox sets the same
  limits. The desktop limit keeps the agent from making its desktop the one the lawyer sees.
- **The token** is (B) as above, made by the inner runner from its own token with
  `DISABLE_MAX_PRIVILEGE`: `SeChangeNotifyPrivilege` is the one privilege left, and it must stay
  (bypass traverse, §3 of the plan). The inner runner sets `SetErrorMode`; the agent inherits
  only its three pipes. The agent runs as the inner runner's own user, so before it starts the
  inner runner gives its own process, thread and primary token a protected DACL naming `SYSTEM`
  and the lawyer alone, with `OWNER RIGHTS` held to `READ_CONTROL`: the agent can't open its
  Medium, unrestricted token (the harness's `runners` row first found query and duplicate open).
  It makes the agent's profile folders part by part relative to each
  parent's handle, never through a link.
- **The pipes** (D11's mitigation). The outer makes four named pipes per launch (`ctl`, the
  agent's stdin, stdout and stderr), one instance each, made first or refused, local clients
  only, with an explicit protected DACL giving the lawyer and the pool user read and write (no
  `WRITE_DAC`, `WRITE_OWNER` or new instance) and no one else anything, so another matter's
  agent gets error 5, and a Medium label with no-read-up and no-write-up, so a Low agent of the
  same matter can't open one by name either. Before it sends a byte it checks each client is
  the inner runner's process, and that process's token user the pool user. The launch goes over
  `ctl`, never the command line, which `CreateProcessWithLogonW` caps at 1024 characters.
- **The environment.** `USERPROFILE`, `HOME`, `HOMEDRIVE`/`HOMEPATH`, `APPDATA`, `LOCALAPPDATA`
  and `TEMP`/`TMP` point under the state folder (the session's temp folder when the policy has
  one). A variable that points into the lawyer's profile outside the paths the launch grants is
  dropped, and so is a `PATH` or `PSModulePath` entry there outside the runtime paths. git uses
  OpenSSL (`GIT_CONFIG_COUNT=1`, `http.sslBackend=openssl`): Schannel fails under any
  restricting-SID token. System32's `curl.exe` (`SEC_E_NO_CREDENTIALS`) and PowerShell's web
  calls fail for the same reason, a known breakage.
- **The CA bundle.** Codex's rustls loads no roots from the Windows store under the token, so
  `CODEX_CA_CERTIFICATE` and `SSL_CERT_FILE` name `<data_dir>\sandbox-ca\roots.pem`: the local
  machine's and the current user's `Root` stores (group policy and enterprise included), less
  anything in `Disallowed`, anything not self-issued and roots their store properties limit. It
  is written once per app start and when the pool changes, atomically, owned by the lawyer,
  readable by each pool user and writable by none. A firm's CA files (the launch's `firm_ca`,
  which R17 fills from `network.ca_certs`; never the spec's own `SSL_CERT_FILE`) are appended to
  it, each certificate once, only a CA's and none in `Disallowed`, into a merged file beside it
  with the same descriptor. `SSL_CERT_FILE` makes every OpenSSL tool in the sandbox trust exactly
  the machine's roots and the firm's, which is intended. The egress proxy (S16) tunnels TLS and
  never terminates it, so it has no CA to add.
- **M3.** Tier B never grants on, links from or points at the lawyer's `~\.codex`. Before any
  entry is applied, the launcher (PR6, R17) calls `codex_link_in_policy`: a policy path lying in
  it is refused, and so is a credential link that is a symbolic link or is `~\.codex\auth.json`
  by file ID (a hard link to it). The outer runner checks the same again before logon, and also
  refuses a `CODEX_HOME` in it. The path checks compare text: a short 8.3 name of the lawyer's
  profile folder isn't caught, a known gap.

### The network (PR7)

`crates/sandbox/src/windows/wfp`. A pool user's agent reaches `127.0.0.1` on 46100-46199,
where its launch's egress proxy and listed ports are, and nothing else. Rulings: the
orchestrator's of 2026-11-07 on the security review (no control grants, static permits, residuals,
the range moved).

**The range** (`ports::TIER_B_PORTS`, 46100-46199) holds a tier-B launch's listeners and nothing
else: its proxy's agent listener and R24 relay (`ports::bind_loopback(ListenerFor::TierB)`,
`ProxyConfig::listener`), and what a tier-B launch claims (`claim_port`, `claim_port_at`). A
tier-C launch's listeners, and the proxy's browser listener on any tier (it takes no token), bind
outside it (`ListenerFor::Other`: port 0, asked again when the OS hands out a port in the range),
so no pool user's static permit reaches them when a tier-C browser launch runs on a tier-B
machine. It sits below 49152,
where Windows' dynamic range starts by default, so the OS never hands its ports to another
program's port-0 bind, and clear of the listeners known below it (WinRM 47001, the ASP.NET state
service 42424, EtherNet/IP 44818, BACnet 47808, SQL Server's browser 1434) and of IANA's
registrations. The browser claims outside it (`claim_browser_port`, `claim_browser_port_at`;
agent-browser's daemon port is 49152-65534), so Chrome's debugging port, from this or a concurrent
tier-C session, never lands there. Set-up and every launch check the machine against the range
(`wfp::check_ports`) and refuse tier B, named, on an overlap:
- **Windows' TCP dynamic range**, read from WMI: `MSFT_NetTCPSetting` in `ROOT\StandardCimv2`,
  `DynamicPortRangeStartPort` and `DynamicPortRangeNumberOfPorts`, every setting's (the class
  behind `Get-NetTCPSetting`; Microsoft documents it under "MSFT_NetTCPSetting class"), on a
  thread of its own, capped at 5 s (no answer fails closed, `PortsUnchecked`). A VM row measures
  its cost unelevated, which decides whether a host process should cache it. For IT:
  `netsh int ipv4 set dynamicport tcp start=49152 num=16384` restores the default.
- **Excluded port ranges** (Hyper-V, WSL and Docker reserve blocks, sometimes below 49152).
  Windows publishes no call that lists them: `netsh int ipv4 show excludedportrange` reads the
  TCP/IP stack's private store, and the documented calls, `CreatePersistentTcpPortReservation`
  and `LookupPersistentTcpPortReservation`, only make or look up one exact range. So each port of
  the range nobody listens on is test-bound on `127.0.0.1` and `::1`; `WSAEACCES` marks it
  excluded (`ports::refused_ports`).
- **At each launch, the listeners**: every TCP listener in the range, IPv4 and IPv6, any address
  (`GetExtendedTcpTable`, `TCP_TABLE_OWNER_PID_LISTENER`), must be this process's: the proxy
  and the relay run in the host. A program-file match would pass another Windows user's Clauseo,
  or this program started suspended and injected into, so it isn't one. Anything else,
  `http.sys` as System included, refuses the launch (`NetworkGap::ForeignListener`, naming the
  program from `QueryFullProcessImageNameW`). A port that refused its test bind is re-checked
  against the listeners read after the binds, so a listener that came up meanwhile is named as
  one.

**Set-up** (elevated, step 8a, between the fence and the journals) writes, in one WFP transaction:
Clauseo's persistent provider and sublayer (weight `0xFFFE`); every filter already in the sublayer
deleted, foreign ones included; six block filters; and one static permit per pool user. The blocks
cover every connect over IPv4 and IPv6 (loopback and the range included), every inbound connection
that isn't loopback, and ICMP at resource assignment, each clearing its action right. Each block's
`ALE_USER_ID` condition matches `NOT_EQUAL` over `(D;;CC;;;<group>)(D;;CC;;;<each pool
user>)(A;;CC;;;WD)(A;;CC;;;RC)(A;;CC;;;AC)(A;;CC;;;S-1-15-2-2)`: it is true when an access check
doesn't grant `CC`, and a deny entry also matches a deny-only SID. So a pool process that makes its
user SID or the group deny-only in a derived token (no privilege needed) is still blocked; review
item 1. The allows keep every other token out of the block: `Everyone`, `RESTRICTED` for a
restricted token's second pass, and the two application package groups for `AppContainer` and LPAC
apps. A non-pool restricted token whose restricting set holds neither `Everyone` nor `RESTRICTED`
fails the second pass and is blocked too; the spike measures this collateral. Each static permit, at
a weight above the blocks, is `127.0.0.1`, TCP, remote ports 46100-46199, with an `ALE_USER_ID`
`EQUAL` allow for that one user: a derived token that escapes the allow loses the permit and falls
to the block. Then come the owner's grants, **read-only, no control**: `READ|ENUM` on the filter
container, and `READ` on our provider, sublayer and filters. There is no ADD, ADD_LINK, WRITE or
DELETE anywhere, so the lawyer can't add, change or remove a WFP filter. Implicit owner rights would
undo that, so set-up reads back, inside the transaction before it commits, the owner of the
provider, the sublayer and each filter: Administrators or SYSTEM, or the set-up fails, named (the
policy "System objects: Default owner for objects created by members of the Administrators group"
set to the creator would make the lawyer the owner). The disclosure is small: the lawyer's processes
can enumerate the machine's WFP filters, which shows other products' filter metadata but gives no
control. Last come four Windows Firewall rules naming the pool: block everything that isn't
loopback, both ways; block loopback UDP; block loopback TCP outside 46100-46199. A re-run or an
extend rewrites it all for the whole pool. A WFP failure fails the set-up at `Step::Network`.

**Measured** (spike run 42351029777, branch `exp-wfp-denyonly` at `7d66a922b`, GitHub's
`windows-2025` and `windows-11-arm` runners, 23 rows passing on each). The spike installs these
filters for a throwaway user and group with `wfp::install`, deletes the firewall rules so only WFP
is measured, and runs a probe under each token. Each non-pool row ran once before the filters and
once after. Results:
- WFP accepted `NOT_EQUAL` over the descriptor, and the launch check passed for the pool user and
  refused the admin.
- With no filters, every token reached, the pool user's deny-only child included.
- With the filters:
  - the plain pool token, the (B) token, a pool token restricted without `Everyone` and a
    Low-integrity pool token were blocked outside the range and reached loopback inside it;
  - **the deny-only-user child and the deny-only user-and-group child were blocked outside and
    inside**;
  - the pool user's AppContainer child was blocked;
  - the admin's own token, the admin restricted with `RESTRICTED`, and SYSTEM still reached (no
    collateral there).
- Harness limits: a restricted token holding neither `Everyone` nor `RESTRICTED` can't start a
  program at all (`0xC0000022`), before or after the filters. The admin's AppContainer child
  failed to reach 1.1.1.1 before the filters too (runs 42351029777 and 42354843371), so the pool
  user's AppContainer row proved nothing then. Run 42356413018 ran the control in a registered
  AppContainer profile with `internetClient` and `privateNetworkClientServer`, which Windows
  Firewall needs to let a package out, and it reached before and after the filters. The pool
  user's AppContainer child has not been shown blocked: Windows refused to start it
  (`CreateProcessW` with the AppContainer attributes: access denied, run 42357105557, with or
  without the user's profile loaded), while the admin's starts. A step that never ran counts as
  information, never a pass. The argument stands on token semantics: a lowbox token keeps the
  user SID and groups, and its package check must pass as well as the normal access check,
  whose first pass hits the deny entries for the pool user and group, so the block's condition
  holds.
- A derived token needs a default DACL its process can open (for example `Everyone`) to start;
  a hijacked agent can set one on its own derived token, so the rows give it one.

**Each launch** (`check_launch`, as the desktop user) checks before the agent starts, and adds
nothing:
- the provider and the sublayer are there;
- the sublayer is at set-up's weight;
- every filter in the sublayer is one of set-up's, exactly: key, provider, layer, action, weight,
  every flag bit (a disabled filter refuses), match types and conditions;
- every block denies the matter's pool user;
- the user has its static permit, and every permit names a user the blocks deny (the pool);
- the machine's dynamic and excluded port ranges, and the listeners in the range ("The range").

A foreign filter, a change, a missing object or a refused read refuses the launch with
`Error::NetworkUnfiltered` (`sandbox.setup_required`; set-up deletes foreign filters). So do, with
`agent.launch_failed`: `Loopback::Any` (D10: the browser is off on tier B), a port outside
46100-46199, WFP not answering, or the range overlapped, reserved or held by a foreign listener. The
ports `Policy::proxy_ports` and `Loopback::Ports` name narrow nothing at the WFP layer, but are
still checked to be inside the range, so a launch never counts on a port the firewall blocks. No
agent runs unfiltered.

**Inside the range, the listeners' secrets are the check.** Every pool user reaches every
listener on `127.0.0.1:46100-46199`, other matters' included. S16's proxy demands its per-launch
token, and R24's relay its token file. **Accepted risk:** a listener an agent makes in the range
(or one it puts on another matter's listed port before that port is bound, review item 7; S16's
`SO_EXCLUSIVEADDRUSE` is out of PR7's scope) is reachable from another matter's agent: a
collusion channel between two hijacked agents, like the pipes (D11). **Accepted risk:** a foreign
program that binds a fixed port in the range after a launch's check is reachable from that
launch's agent until the next launch refuses; the range is out of the OS's hands, so only a
program asking for that port by number can do it.

**Defence in depth, dropped.** A filter at `ALE_AUTH_RECV_ACCEPT_V4` that blocks a connection
whose remote user (`FWPM_CONDITION_ALE_REMOTE_USER_ID`) is a pool user to any local program but
the host (`FWPM_CONDITION_ALE_APP_ID`) would cover a foreign listener wherever it lands. Windows
leaves the remote user empty for a loopback accept: in spike run 42354843371 (`exp-wfp-denyonly`,
`windows-2025` and `windows-11-arm`) the filter didn't stop the pool user reaching a foreign
listener, while the same rows' controls passed. The spike's rows stay on that branch as the
record.

**When the firewall fails.** WFP is the boundary: any WFP call that fails, the Base Filtering
Engine not running included (named for IT: set BFE to start automatically), fails the set-up, so
the machine stays on tier C (`SetUpFailed`). The firewall is the second layer: once WFP's filters
are in, no firewall failure fails the set-up. Set-up goes on with WFP alone,
`SetupReport::network` names the firewall as ineffective with its cause, and the set-up logs it.
The causes:
- Group Policy keeps local rules from taking effect. For firm IT: add the same rules by Group
  Policy, or let local rules apply (`AllowLocalPolicyMerge`).
- The Windows Defender Firewall service (`mpssvc`) isn't running. IT can set it to start
  automatically.
- Windows Firewall is off for a current profile, as when a third-party firewall owns it. IT can
  add the same rules there, or turn Windows Firewall back on.
- A firewall call fails otherwise; the call and its code are named.

The firewall's `LocalUserAuthorizedList` can only allow, so a deny-only derived token slips past
the firewall rules; WFP's blocks still catch it. `setup-result.json` doesn't carry the firewall's
state; the host can show it later through R17 if IT asks.

**Inbound loopback** to a pool user's own listener is not blocked: whoever connects is filtered on
its own connect side. No listener rule: `ALE_AUTH_RECV_ACCEPT` never saw the loopback peer in
PR0 (c), and the browser waits for its own design.

**Accepted risks: traffic other accounts carry.** No `ALE_USER_ID` condition sees these. Each
VM row measures it.
- **Name resolution.** The DNS Client service resolves names as NETWORK SERVICE, so a hijacked
  agent can still look names up: a DNS exfiltration channel, which Codex shares. Direct DNS (port
  53 to any resolver) is blocked. For firm IT: firms that need to close DNS exfiltration can point
  the machine at a filtering resolver.
- **SMB.** The SMB client connects from the System process. The restricted token fails network
  logon, so what leaks is the server name and the TCP connect.
- **WebDAV.** Opening `\\host@SSL\DavWWWRoot\<path>` makes the WebClient service (LOCAL SERVICE)
  send HTTPS to any host, and the path goes out before any authentication. Windows 11 starts
  WebClient on demand. For firm IT: disable the WebClient service by Group Policy if you don't use
  WebDAV. The row runs on a Windows 11 copy if there is one, else on win-01 (Server 2025, no
  WebClient), and says which.

**Smaller notes.** `::1`, `127.0.0.2` and the rest of `127.0.0.0/8` stay blocked. A socket opened
under anonymous impersonation is blocked too: whether WFP reads the process's token (a pool user) or
the thread's (the anonymous logon, which holds no `Everyone`), the allow doesn't grant. Only a
machine with `EveryoneIncludesAnonymous = 1` that WFP reads by thread would let it through; a VM row
measures it. Removal (`wfp::remove`, which `--remove` runs once every pool account is gone and a
second sweep has stopped, elevated, every process still running as a pool SID (one that won't stop
ends the run at `RemoveStep::Strays` with exit 4, `RemovedPartly`, the network still on, for a
re-run), since `stop_strays` only logs a process that won't stop) deletes every filter in the
sublayer, foreign ones included, every key set-up writes, the sublayer and the provider, in one
transaction; then it takes back the owner's read grant on the filter container, and removes the
firewall rules; a firewall failure there is reported, not fatal, as at set-up. It can run again
after a partial removal.

**For the host (R17 PR-E).** Per tier-B launch, once the policy is built and before the runner
starts: `check_launch(&pool_user_sid, &policy)`; on `Err`, don't start the agent, and map
`Error::NetworkUnfiltered` by `Error::code`.

### Admin copy refresh

The elevated exe runs only from its admin copy,
`%ProgramFiles%\Clauseo\Sandbox\clauseo-sandbox-setup.exe`
(PR 1 places it; PR 2's `--adopt` records `AdminCopySha256`, `AdminCopyVersion` and
`UserInstallDir` in `HKLM\SOFTWARE\Clauseo\Sandbox`). A per-user install updates itself without
elevation, so the copy goes stale. A SYSTEM scheduled task refreshes it without a prompt, and
only from an exe the updater key signed.

**The task.** `--adopt` registers `\Clauseo\SandboxRefresh` through `ITaskService` (`register_task`,
`SystemTask::Refresh`). It runs as LOCAL SYSTEM. Its one action is the admin copy's path, resolved
from `FOLDERID_ProgramFiles` and quoted (it has a space), with `--refresh` and no other argument. It
starts at the record owner's logon and on demand, needs no network, stops after 5 minutes, and never
runs twice at once. The task and its folder carry
`O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;<owner>)`: the owner may read and execute the task
(`FRFX`, `FILE_GENERIC_READ | FILE_GENERIC_EXECUTE`), and can't change or delete it. Starting a
task needs execute access, as Microsoft's Win32 page on task security says: with `FR` alone, the
owner's `schtasks /run` was refused ("Access is denied", CI run 42416074600). Execute access also
ends a running task (`SchRpcStop`), so the owner can end a stop or a refresh. That is accepted:
ending a task is denial of service only, which code running as the lawyer can do to its own
sessions anyway. An ended refresh keeps the old copy, and an ended stop can't pass for a finished
one (only a completed stop writes `StopSequence`, "Stopping the pool's processes"). The elevated test
`elevated_the_owner_can_start_and_end_a_task_it_can_read_and_execute` shows both as a fresh
standard account: `schtasks /run` starts the task and `schtasks /end` ends it. The folder gets that owner and DACL
when it is made. A folder already there is kept, and given them again, only when its owner is
Administrators or SYSTEM, which only an administrator can set. Anyone else's (a standard user may be
able to make `\Clauseo` first), or one whose owner can't be read, is deleted with every task and
folder in it, then made again (`task_folder::prepare`). The app asks for a run after an update (`request_admin_refresh`,
`IRegisteredTask::Run`, unelevated). Only
`--remove --uninstall` deletes the task and the staging folder ("Two removals"); plain
`--remove` keeps them with the copy. `--adopt` registers the task before it writes the record's
admin-copy values, so a failed registration leaves no valid record: `--adopt` exits 3, and the
launcher's check finds the copy missing.

**The run.** `--refresh` takes nothing from the user except the files in the recorded folder:
no path argument and no environment. `UserInstallDir` is untrusted: the lawyer can re-point any
folder of it through a junction at any time, or re-run `--adopt` with another folder. The run:

1. refuses unless it runs as LOCAL SYSTEM from the admin copy's path;
2. refuses a build whose key is the placeholder, before reading anything. It then tries the
   set-up lock (the named mutex the set-up, `--remove` and `--adopt` take) without waiting, and
   holds it to the end of the run; held elsewhere, it exits 44 having read nothing. `--adopt`
   holds the same lock from its record checks through its hash and record write, so a refresh
   never swaps the copy between the two;
3. refuses an install folder that isn't inside the copy owner's profile (`ProfileList`'s
   `ProfileImagePath` for `AdminCopyOwner`; a temporary profile counts as none). For a
   `machine` owner, the folder must instead be one only SYSTEM, Administrators or
   `TrustedInstaller` can change (`level_safe`, `--adopt --per-machine`'s test). It reads
   `AdminCopyOwner`, not `OwnerSid`, because the in-app `--remove` deletes `OwnerSid` and keeps
   the copy and the task;
4. opens the drive's root, then each folder of the path relative to its parent's handle
   (`NtCreateFile` with `RootDirectory`, `OBJ_DONT_REPARSE` and `FILE_OPEN_REPARSE_POINT`),
   refusing a reparse point at every level. It then opens `clauseo-sandbox-setup.exe` and its
   `.sig` relative to the last folder, as whatever is there (no `FILE_NON_DIRECTORY_FILE`, so a
   directory junction opens as itself and is refused by its tag rather than denied at the open),
   and refuses a reparse point, anything but a regular file, a second hard link, or more than
   16 MiB (exe) or 4 KiB (`.sig`). Every open is for reading:
   SYSTEM never writes or deletes anything in the folder;
5. copies the exe into `Sandbox\staging\` (owner SYSTEM, protected SYSTEM-only DACL, read back),
   and verifies the staged bytes, never the user's file, so a swap after the check changes nothing;
6. requires the staged image's CPU type (its PE `Machine`) to be the running copy's, so a signed
   exe for another CPU never replaces it;
7. checks the versions ("Versions" below), so an older signed exe can't roll the copy back;
8. stops when the staged SHA-256 is already recorded;
9. otherwise opens `Sandbox\` for adding a file (`FILE_WRITE_DATA | FILE_ADD_FILE`), sharing
   read, write and delete, and holds it until the renames are done: a user can read the folder,
   and a handle that shares read only would fail a rename into it, so one already open stops the
   run (exit 43) before anything moves, and none can be opened while it is held. It then renames
   the running copy into staging, which Windows allows for a running exe, and moves the staged
   file into place with the admin copy's DACL. Only then does it write the new
   `AdminCopyVersion` (the signed version, as signed), then the new hash. The set-aside copy
   stays in staging until the next run deletes it. That run starts from the new copy, which shows
   the new copy runs.

**Crash safety.** The record never names a hash the admin path doesn't hold: until the last step
it holds the old hash. A crash after both renames leaves the new file under the old hash, which
the launcher refuses. The next run then finds the admin path already holding the staged exe and
writes its version and hash without another swap. The version goes first, so a recorded new hash
always has its version. A failed move puts the old copy back.

**Accepted: the gap between the two renames.** A crash, or the task's 5-minute kill, between the
two renames leaves no exe at the task's path. No run can heal that, because the task can't start.
The launcher then finds the copy missing (`AdminCopyMissing`), and the app's words lead the
lawyer to the installer's repair, which places a copy again. The old copy is still in staging
under its set-aside name, so repair can restore it. The gap is as narrow as it can be: two
write-through renames back to back, with nothing between them.

**The signature.** The `.sig` is Tauri's: base64 of a minisign signature file, prehashed (`ED`),
checked with `minisign-verify`. The trusted comment is the signer's once the global signature
verifies. tauri-cli 2.12 writes it as `timestamp:<seconds>\tfile:<basename>\tversion:<semver>`.
It is read as tab-separated `key:value` fields, matched by key in any order. A field without
`:` or a key given twice is refused. `file` must be exactly `clauseo-sandbox-setup.exe`, so a
signed installer can't stand in for the exe. `version` must be present. `timestamp` is not
checked.

**Versions.** The signed `version` is the release's semver (release.yml's `$VERSION`, such as
`1.4.0` or `1.4.0-beta.1`), and `build.rs` writes its core into `FILEVERSION` as `X.Y.Z.0`.
- The staged exe's `FILEVERSION` must be the signed version's core with a fourth part of 0. So a
  signed `0.1.0` matches `0.1.0.0`, but not `0.1.0.1` or `0.1.1.0`. The signature thus binds
  the exe's own version.
- The signed version is then ordered against `AdminCopyVersion` by semver precedence
  (`version_not_older`). A prerelease is older than its release, so `1.4.0-beta.1` can't replace
  `1.4.0`, but `1.4.0` replaces its beta. Build metadata doesn't count. `release.yml` signs only
  plain `X.Y.Z` today, so the prerelease ordering is latent until a prerelease is signed.
- An older version is refused. So is an equal version whose bytes differ from the copy in place.
- A leading `v` is dropped. A record's `X.Y.Z.0` reads as the release `X.Y.Z`. Anything
  unreadable, a non-zero fourth part included, refuses the run.

**The key.** `crates/sandbox/build.rs` builds the key in: `CLAUSEO_UPDATER_PUBKEY`, else
`plugins.updater.pubkey` from `tauri.conf.json`, else the placeholder `AC986E7C6BA67AB9`. The
placeholder has an all-zero key and is refused by its id. A release build
(`CLAUSEO_RELEASE_MODE=release`, any profile) fails unless the key is the pinned production key,
`PRODUCTION_KEY_ID` in `refresh/key.rs`. That id is unset until #527 adds the production key, so
until then every release build fails. Two things enforce it:
- `build.rs`, with the mode `release.yml` exports as `CLAUSEO_RELEASE_MODE` in its build
  legs' env (`release`, `test` or `dry`, from `check`'s mode; #648, which also exports the test
  key as `CLAUSEO_UPDATER_PUBKEY` in a test run). Any other value, the empty string included,
  fails the build, so a typo can't turn the guard off. Unset is a local build.
- On msvc the exe's `VERSIONINFO` carries the key's id, as the string `UpdaterKeyId` (16 hex
  digits). `release.yml`'s assemble job reads it from each shipped setup exe, without running
  it (which would prompt), and compares it with the mode's key (#648). Until this PR merges,
  an exe without the string is only a warning there; after it, a failure.

Each refusal keeps the old copy, writes one Event Log line (source `Clauseo Sandbox`, naming no
user path beyond the install folder), and exits with its own code. The codes start at 32, clear
of the set-up's (`RefreshExit`).

### Stopping the pool's processes

The runner's sweeps (#649) stop a matter's processes as its pool user. A process that shut its
own DACL survives them and can stay alive into the next logon. SYSTEM, with `SeDebugPrivilege`,
can stop it whatever its DACL says.

**The task.** `--adopt` registers `\Clauseo\SandboxStop` right after `\Clauseo\SandboxRefresh`
(`register_task`, `SystemTask::Stop`), the same way: LOCAL SYSTEM, the same descriptor (the
owner may read and execute it, which starts and ends it), one instance at a time. Its action is the admin copy with
`--stop` and no other argument. It runs only on demand and stops after 2 minutes. A failed
registration fails `--adopt` before the record's admin-copy values are written, as for the
refresh task. `--remove --uninstall` deletes both tasks, after the values and before the admin
folder (`refresh::TEARDOWN`). The folder rule above has two accepted outcomes. A user-made
`\Clauseo` holding a task administrators can't delete fails the delete, and so the registration,
with that error: a user who could make the folder can block the install, but not take it over.
A folder an administrator made, owned by that administrator's own SID rather than
Administrators, is deleted and made again, which loses nothing.

**Per machine.** A per-machine `--adopt` has no owner to name in the descriptor, so it registers
neither task. The refresh task stays per-user only: IT updates a per-machine install. The stop
task is registered by the elevated set-up instead, once it has written the record with the
owner as `OwnerSid`, when the record's `AdminCopyOwner` is `machine` (`late_steps::stop_task`).
It is the same registration as `--adopt`'s: same principal, argument, limits and descriptor,
with `OwnerSid` as the owner who may read and execute, and so start and end, it. Every set-up of the owner registers it
again, replacing the one there. A new owner's set-up, after a removal, replaces the previous
owner's task, so only the new owner can start it. A failed registration fails the set-up at
the step "registering the sandbox's stop task", rolled back like any other step. `--remove`
deletes the task just before the record (`RemoveStep::StopTask`); a failed deletion is a partial
removal, with the record kept for the next run. `--remove --uninstall` also deletes it with the
admin copy.

**The run.** `--stop` (`setup/stop.rs`):

1. refuses unless it runs as LOCAL SYSTEM from the admin copy's path;
2. reads `PoolSids` and `OwnerSid` from `HKLM\SOFTWARE\Clauseo\Sandbox`, which only the
   elevated set-up writes. No SID comes from an argument, the environment or the lawyer's data
   folder. No record, or an empty pool, is nothing to do;
3. holds every listed SID to the pool-user check before a profile delete (`check_pool_sid`):
   a local account of this machine, not built in, not SYSTEM, not the owner, named
   `ClauseoSbNN`. One that fails stops nothing;
3a. reserves this run's number before its first process snapshot: under the set-up lock (the
   same 30 s wait), it checks the record still holds the pool, reads `StopNext` (`REG_QWORD`, 1
   when absent), keeps it as `n` and writes it one higher, then gives the lock back. A lock that
   stays held, a record gone or changed, or a write that fails is 50; a read that fails is 46.
   Either way nothing is stopped. The task runs one instance at a time, so no two reservations
   overlap. A run that later ends non-zero, or is ended, has still used its number;
4. passes over the processes until one pass finds none of the pool's. Each pass takes a Toolhelp
   snapshot and opens each process with `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE` (and
   `PROCESS_SUSPEND_RESUME | READ_CONTROL` where it can). Once every handle is open it reads the
   kernel's process list (`WTSEnumerateProcessesExW`, level 1), which gives an administrator each
   process's SID; a pid can't be reused while a handle to it is open, so each entry is the process
   its handle is on. A pool user owns its token and can drop SYSTEM from the token's DACL
   (`SeDebugPrivilege` passes the process's DACL, not the token's). The list reads the token
   too: such a process gets no SID from it (CI run 42416074600), and its token can't be read
   either, so the process object's owner names it, read with `READ_CONTROL` under
   `SeDebugPrivilege`, which the token's DACL can't take away. The token's user is a
   cross-check: when it differs from the list's, the process's user is unknown. With no list
   entry, the token's user counts. With neither, the owner (which a standard user can't set to
   another account) may add the process, never clear it: an owner that is a pool SID gets it stopped, and any other owner leaves it unknown, since
   an owner is the creator, not the user (an elevated administrator's processes are owned by
   Administrators). A process whose user no source gives is never stopped but counts as unknown
   (48, every pool user marked), with one exemption: the kernel's minimal processes (System,
   Registry, Memory Compression, Secure System). Such a process can't be opened with
   `PROCESS_TERMINATE`, the list puts it in session 0, the query-limited handle (which a
   protected process grants) shows it has no image file (`ProcessImageFileName` is empty), and it
   isn't a subsystem process (`NtQueryInformationProcess(ProcessBasicInformation)` with
   `PROCESS_EXTENDED_BASIC_INFORMATION`, `IsSubsystemProcess` clear; a WSL 1 pico process has no
   image file either, and a query that fails counts as unknown). Any image at all is unknown,
   wherever it lies and whoever signed or owns its file. Why: a pool user's own scheduled task
   (S4U, or "run whether logged on or not") runs in session 0 and can start a Windows-signed
   System32 binary as a protected process (PPL); its token's default DACL can shut SYSTEM out of
   the list, and SYSTEM can open a PPL with neither `READ_CONTROL` (so the owner is unread) nor
   `PROCESS_TERMINATE`. SYSTEM can't kill it whoever it belongs to, so the answer is 48 and a
   reset before the pool is used again, not a kill. An earlier exemption for system images (under
   the Windows or a Program Files folder, owned by SYSTEM, TrustedInstaller or Administrators)
   let exactly that process through, and is gone. The run's log line then lists the unknown
   processes' image file names (the last path component only, never a folder, a SID or a pid;
   each once, at most 5 and "and N more", `unread` when there is no path) so firm IT can tell
   support which product it is. A process the list didn't list is unknown too. When the list
   can't be read at all, that exemption goes too: the run ends at 48 unless the token, or a pool owner,
   named every process. Pid 0 and 4 and this process are never looked at. Each pass adds the pool
   users it found to `ResetSids` (step 6) before it stops anything. The pass suspends every pool user's process it found (`NtSuspendProcess`),
   then terminates each through its handle, so a parent can't start another between two
   terminations. Nothing resumes a process: it is terminated next, and one that won't end stays
   suspended, a pool user's, until the reset. Each terminated process is waited for up to 1 s,
   and the waits stop 15 s after the passes' 60 s, so the run ends inside the task's 2 minutes
   with the lock's 30 s wait. What the pass after 60 s still finds is left (48), its users
   marked: so a pool user's process the list names but SYSTEM can't terminate (a PPL) never
   ends a run at 0;
5. cancels every BITS job a pool user owns (`EnumJobs(BG_JOB_ENUM_ALL_USERS)`, each job's
   `GetOwner`), at the impersonate level on the manager, the enumerator and each job, as the
   runner's end sweeper does. A pool user's BITS job can't run (the runner's test shows it, #649)
   but stays queued; the user's own sweeper can't cancel it once the user is logged off
   (`ERROR_NOT_LOGGED_ON`, 1245), which is what `request_pool_stop` is called for. This step
   runs even when no process was found. Other users' jobs are left alone;
6. adds each pool user it found running, or owning a job, to the record's `ResetSids`
   (`REG_MULTI_SZ`, a value appended to the set-up's, deleted with them by `--remove`), under
   the set-up lock with a 30 s wait. Each user is added as soon as a pass or the BITS step finds
   it, so a run the task's limit kills has still marked every user it saw. A pass that finds a
   process of unknown user marks every pool user at once, since it may be any of theirs (the
   run still ends at 48). Once the lock stays
   held or a write fails, no more users are tried (49), so the wait comes once at most.
   Marking first means a held set-up lock can delay the first kill by up to 30 s, once per run;
7. only on a run that will end 0 (every pool user's process gone, every pool BITS job cancelled,
   `ResetSids` written), under the set-up lock, re-reads `PoolSids` and checks it is still the set
   of SIDs this run worked from, then writes `StopCompleted` (`REG_QWORD`, a `FILETIME`: when the
   last clean observation began, the earlier of the pass that found no pool user's process and
   the BITS step's enumeration, so a pool process started after it isn't claimed covered), and
   only then `StopSequence = n` (`REG_QWORD`), the number step 3a reserved. A later stop's larger
   `n` overwrites it; it is never lowered. `StopNext`, `StopSequence` and `StopCompleted` are
   set-up values outside the record proper, deleted with the record by `--remove`. Every write the
   stop makes opens the record's key without making it, so a stop still running after `--remove`
   can't bring the key back. A run that is ended, interrupted by the task's limit, or exits
   non-zero never writes the pair and leaves an earlier one as it is. A record gone or changed, a
   `StopSequence` already past `n`, a write that fails, or a lock that stays held is 49, logged. A
   run with no pool recorded has nothing to clear and writes nothing.

**Only `StopSequence` counts.** The owner holds execute access on the task, which Run needs
(CI run 42416074600) and which also ends a task, so code running as the lawyer can end a stop
mid-pass. An ended stop can't pass for a finished one. The rule: the follow-up's unclean-session
marker records `StopNext` as it reads it when it is written (M). It must read `StopNext` under
the set-up lock, so it can't interleave with a reservation's read-then-write and record a stale
M. The F4 refusal and the marker clear only when `StopSequence >= M`: a stop reserved its
number after the marker existed and then completed, so its observation began after the marker.
Reserving before observing closes the race where a stop between its clean pass and its write
would clear a marker made in between. Nothing clears on the task's end or its exit code, and
nothing trusts the clock; `StopCompleted` is the last clean pass's time, for diagnostics.
`--remove` deletes `StopNext`, `StopSequence` and `StopCompleted` with the record, and should
clear the markers too, so no marker outlives its set-up. That wiring lands in the follow-up PR with the runner (#649); this
PR writes the values. Ending a stop is denial of service only: the pool users stay refused until a
stop completes.

It logs one Event Log line with counts only (processes stopped, passes, left, unknown, jobs
cancelled and not), and says so when a pass couldn't read the process list. Its exit codes follow the refresh's (`StopExit`):

| Code | Meaning |
|---|---|
| 0 | every pool user's process found was stopped (or none ran), those users are marked, and `StopCompleted` then `StopSequence = n` are written |
| 45 | not LOCAL SYSTEM, or not run from the admin copy; nothing stopped |
| 46 | the record can't be read, or its `PoolSids`, `OwnerSid` or `StopNext` is malformed; nothing stopped |
| 47 | a SID in `PoolSids` isn't a pool user; nothing stopped |
| 48 | a pool user's process or job was left, a user or owner couldn't be read, or a list failed |
| 49 | the processes were stopped, but their users couldn't be added to `ResetSids`, or `StopCompleted` and `StopSequence` couldn't be written (lock held, write failed, record gone or pool changed, sequence past `n`) |
| 50 | `StopNext` couldn't be reserved (lock held, record gone or pool changed, write failed); nothing stopped |

**The reset.** The stop doesn't reset the profiles itself. A reset sets a new password, which
only the owner's `secrets.bin` may hold (DPAPI, as the owner). Its open-session check and its
`LocalLow` denies also read and write the owner's data folder as the owner, and the task has
no owner token. So the next set-up resets each user `ResetSids` lists, even one a matter
holds, once no session is open (D-B6), then takes it off the list. While a session is open, or
the open sessions can't be read, the users stay listed for a later set-up. Keeping a listed
user from its matter's next launch comes with the runner's wiring (#649).

**Misuse.** The owner's read and execute entry lets any code running as the lawyer start the
task. All it can do is stop the sandboxed agents and mark their users for a reset, which is no
more than quitting Clauseo does. It takes no argument, so it can't be pointed at anything else.
The same entry lets that code end a running stop or refresh (execute access ends a task, CI run
42416074600): denial of service only. An ended refresh keeps the old copy. An ended stop never
writes `StopSequence`, so it can't pass for a finished one.

**Speed.** The launch path doesn't change. The task runs only when a sweep saw a pool user's
process it couldn't stop, or once at Clauseo's start after an unclean session
(`request_pool_stop`, which returns whether the run was queued). Both callers land with #649.

**What the app shows.** After a run that stopped something: "Clauseo stopped an agent that
tried to leave the Safe workspace". The app thread owns the final copy and its Hindi.

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
- Uninstall (S15 PR4j, "Removal" above) removes, for each pool user, its `LocalLow` deny, its
  profile and then its account, then `DeviceSync`'s deny, the ledger's entries, the group and the
  record. Deleting the account first would leave its profile folder and `ProfileList` key behind.
- The token, the default DACL and the desktop come from spikes on Windows Server 2025; Windows 11
  and Windows 10 22H2 are unmeasured.

## Status

proposed (draft; S15's later pull requests complete it)

## Date

2026-11-05
