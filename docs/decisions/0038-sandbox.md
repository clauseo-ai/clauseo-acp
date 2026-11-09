# 0038: The sandbox crate, its policy, and Seatbelt on macOS

## Context

Stage C puts every agent in an OS jail where the machine can give one (synthesis §2.3; rulings 18,
22, 24, 25, 27). The new core crate `sandbox` (S14) holds the policy, the walk of the matter, tier
detection, the denial types, and the macOS jail. S15 adds Windows, S16 the egress proxy, R17 wires
the crate into `host`, R18 turns denials into events and cards.

Seatbelt gives a launcher no callback when it refuses something. A spike on this Mac mini (macOS
27.2, build 26B123, arm64) checked the unified log instead:

- `(with message "<tag>")` is accepted on `(deny default ...)` and on single deny rules, and the
  tag arrives as the last line of the log line's `eventMessage`; a rule without it logs no tag.
- `log stream --style ndjson` with `eventMessage CONTAINS "clauseo-"` saw no other app's lines in
  60 s (a `sender == "Sandbox"` predicate alone saw 95 a minute). The first output line is not
  JSON. Process name and pid are only in the message:
  `Sandbox: <name>(<pid>) deny(1) <operation> <target>`.
- From process start to the line read: p95 18 to 28 ms, max 51 ms over 20 runs on a busy machine.
  RSS 3.1 to 3.6 MB; 0.72 s of CPU over 1,060 lines; all 1,000 denials of a burst arrived.
- One process repeating a denial is folded into `N duplicate reports for ...` about 2 s later.
- About 4,000 link rules made `sandbox-exec` fail to compile the profile, and 20,000 overflowed
  the argument list (review round 1); 2,000 took 6 s to start.
- `/bin/cat` aborts under `(deny default)` unless `/` itself is readable.

## Decision

**The crate.** `crates/sandbox`, package `clauseo-sandbox`, a core crate (Rohan approves every
change). Its row owns the policy model, tier detection, the per-OS launchers, the egress proxy and
denial detection; per-matter state folders and denial events on the wire are `host`'s (R17, R18).
It may depend on `protocol` and `process` plus the plumbing it records: `thiserror`, `tracing`,
`serde`, `dunce`, and on macOS `async-process`, `async-io`, `futures` and `serde_json` for the
watcher. It never depends on `tauri` or `host`.

**Tiers.** A is Seatbelt (macOS) and later MXC (Windows); B the Windows sandbox user (S15); C none.
`detect()` runs `/usr/bin/sandbox-exec -p '(version 1)(allow default)' /usr/bin/true` once on
macOS. Linux stays tier C with `NoSandbox::UnsupportedOs` (shared ruling 6): bubblewrap is Later,
and `docs/threat-model.md` no longer calls it tier A.

**The policy** (`Policy::builder(matter, state)`): read-write matter, state folder (`TMPDIR` is
`<state>/tmp`) and notes folder; read-and-run runtime paths; read-only paths and allowed folders;
the credential link and the Claude store's names read-write; hidden paths denied after every
allow, with the state folder, notes, credential link, Claude store, runtime and read paths allowed
again after them; proxy ports; the browser's loopback; the Keychain switch. Every path is
canonical. `build()` refuses nesting matters, a state folder in the matter, relative paths, and an
allowed folder that meets a hidden path, the matter, the home folder, `/` or a `.git`.

Because the state folder, notes, credential link, runtime and read paths are allowed again after
the hidden paths (and runtime and read after the matter's read-only rules), `build()` also
refuses (review round 1): a matter, state or notes folder whose own name is a symbolic link
(checked before canonicalising: an agent may have planted one in place of its folder last
session); a matter that equals or holds the home folder; a state folder that equals or holds a
hidden path; a notes folder inside the matter, holding it, or equal to or holding a hidden path; a
credential link outside the state folder; a runtime or read path that is `/` or equals or holds the
home folder, the matter or a hidden path (inside one is fine, which is why they are allowed
again). An allowed folder inside `Documents/Clauseo` that is not a matter is accepted by design:
only "holds the matter" stands for the brief's `Documents/Clauseo`.

**Read-only names and links.** `.codex/`, `.claude/`, `.agents/`, `.clauseo/`, `.mcp.json`,
`CLAUDE.md`, `CLAUDE.local.md` and `AGENTS.md` at the matter root, existing or not; each existing
`.git` at any depth and the folder a `gitdir:` file names inside the matter; over-cap files.

The agent can't create a `.git` (dir or file) anywhere in the matter: one it made would be
lawyer-owned and run as the lawyer under the lawyer's git. Nor a `HEAD`: a folder holding
`HEAD`, `objects/` and `refs/` is a bare repository to git's discovery (`safe.bareRepository`
defaults to `all`), no `.git` name involved. Each is one deny on `file-write*`, like the
`CLAUDE.md` rule below: `(subpath (param "MATTER"))` and a path-free regex, `.git` as any path
name, `HEAD` as the last, each letter a two-case class. It refuses `git init`, `git clone`,
`mkdir`, a `gitdir:` file, hard and symbolic links and renames onto the name (Seatbelt checks a
rename's destination). Names that only contain it (`.gitignore`, `.github/`, `a.git`,
`HEADER.md`, `ahead`) stay writable. The policy cost: agents can't clone or init a repository
into a matter (`git worktree add`, submodules and git dependencies cloned into it fail too); they
work in their own scratch or state folder. Nor can they create, rename onto or write any file
named `head` in the matter (any case, ignorables included); `HEAD.txt` is unaffected.

Case-insensitive HFS+ ignores U+200C to U+200F, U+202A to U+202E, U+206A to U+206F and U+FEFF
when it compares names (TN1150), so `.g`, U+200C, `it` opens `.git` there. Both regexes allow
an optional run of those code points before, between and after the letters, each as its literal
UTF-8 sequence (a bracket range would range over bytes). The run is written `((...)+)?`: on this
Mac Seatbelt read `(x|y)*` as one or more (`/\.g(x|y)*it$` did not match `.git`). Only names
HFS+ would take for `.git` or `HEAD` are refused; Hindi and Persian names with ZWJ or ZWNJ stay
writable (jail-tested), and a matter path holding them changes nothing. `strip_hfs_ignorables`,
beside the policy's name fold, removes them before the fold for the watches' name checks.
The `CLAUDE.md` regex and the root's protected names do not take the ignorables.

At launch the walk that finds every `.git` also lists each folder outside one that holds a
`HEAD` file or link, whole bare layout or lone `HEAD` (the agent could add `objects/` and
`refs/`, or `commondir`), and the folder a `commondir` beside it names inside the matter (git
reads `config` there; one outside the matter, or the matter itself, is left out, as for
`gitdir:`). Names compare without the ignorables, then by the policy's caseless fold on every OS
(NTFS opens `objectſ` as `objects`). Each is read-only with its whole tree, like a `.git`, on
macOS and in the Windows ACL plan (a whole-tree DenyWrite). The cost: a folder with a lone file
named `HEAD` (rare in a matter) is read-only to the agent, and a matter whose root holds one is
read-only entirely.

**The git watch** (`src/macos/git_watch`, the lawyer-side backstop). A name rule sees only the
last name of a create or a rename's destination, so it can't stop what arrives under another
name: a folder already holding a `.git` or a bare layout, made in the state folder (writable,
same volume), and moved into the matter (`git init $STATE/foo && mv $STATE/foo <matter>/foo`,
checked on this Mac); a directory clonefile of one; a symbolic link in the matter to a
repository outside it. The same name denies as the matter's (`.git` and `HEAD`, any case, HFS+
ignorables) also cover the session's temp folder and notes, last of the file rules, so no
repository is made there at all. The state folder is left out: Codex clones its plugin
marketplace with git into `$CODEX_HOME/.tmp/plugins` there. So each jailed launch starts a watch
on the matter, outside the jail, before the agent. It looks for:

- a new `.git` (folder, or file naming a `gitdir:`);
- a `HEAD` file or link in any folder that was not a repository at launch, whatever surrounds
  it; a new repository-shaped folder (`HEAD` with `objects` and `refs`, folders or links, as
  git follows links, or with a `commondir` file) is taken whole, otherwise only its `HEAD`;
- a new symbolic link that leads outside the matter or nowhere, judged without following any
  link: its target is walked from the link's folder one name at a time with `fstatat` from held
  descriptors, and a link on the way counts as leading out (`a -> b/../x` with `b -> .` goes
  to the matter's parent, though `b/../x` reads as `x`); one that ends at another link is
  judged as that one is, up to 8 in a row;
- anything new at a launch repository's `commondir` target (below).

Names compare with the HFS+ ignorables stripped, then by the policy's fold. What was there at
launch stays while it keeps its file ID: the walk's `.git`s and outbound links, and the
repositories, `HEAD` folders (left alone while they stay unshaped) and folders of the watch's
own walk (one more walk of the matter per launch, before the agent runs). How it watches:
`FSEvents` on the matter (file events, no defer, 0.1 s latency, a minimal binding to
CoreServices). Each batch of events is first checked by name alone, listing no folder: a `.git`
anywhere in an event's path, a `HEAD` at its end (unless in a launch folder that held one), or
an `objects`, `refs` or `commondir` arriving beside a `HEAD` ends the session at once, whether
or not the item is still there, so a repository renamed into the matter and out again before
the check still ends it. Only a launch repository that keeps its file ID (the profile makes it
read-only) and the volume set-aside folder are passed by. An item an event reported that is
gone by check time is counted, per watch and for the whole host, and logged; a folder renamed in
and out under a plain name leaves only that count. Past the names, every event re-checks its
parent folder's `HEAD`, so a folder of `HEAD` and `config` that gets `objects` and `refs`
later is caught; a created, renamed-in or cloned folder is scanned, within 20,000 listed
entries for the whole batch: past that the scan stops and the session ends, so a huge folder
can't hold up the watch while the agent runs (the final scan after the kill has no cap). Any
find while the agent may run ends the session, gone or not, and a scan then stops at its first
find, walking no further; the scan after the kill is complete. A folder in the matter that
can't be opened or listed (`chmod 000` hides what is in it; a missing folder, or a network
volume or automount the watch refuses, is no such folder) counts as a find while the agent may
run; the final scan keeps it as stuck (below). Every walk calls the runner's progress hook
(`Watch::on_progress`) once per folder it lists. A dropped or must-scan event, a
changed root or a path outside the matter rescans it; a stream that can't start refuses the
launch ("Clauseo won't start the AI tool: it can't watch <folder> for changes (...)"), as there
is no fallback to timed rescans. The session's **notes folder** (`Policy::notes`) gets the
same: a watch of its own rooted there, with its own stream, canary, 30 s pass and state file
(keyed by the notes path), an empty launch (nothing in notes is the lawyer's to keep: the
profile makes no repository there read-only, so `Launch::keep_repos` is false and even a
repository found there at launch is a find), its items
set aside into the data dir's folder, and the same session: a find there ends the session as
one in the matter does, and the host's start and each launch finish its dead sessions as they
do the matter's. `start` takes the matter's mode from `Policy::matter_watch()`: `Live`, or
`AfterOnly` (no stream, canary or 30 s pass: only the launch's check before and the final scan
after) for a matter no stream can watch (below); the notes watch is always `Live`. The launch
logs the mode and the session record keeps it. Every file operation goes by descriptor: a
path's folder is opened with `O_NOFOLLOW_ANY` and its last name read or changed with the `*at`
calls, a walk holds each folder open while it lists it and opens its subfolders from it, and
renames are `renameatx_np(RENAME_EXCL)`. Scans never follow a link, never enter a `.git`, and
never enter, list or look up in a folder on a volume without `MNT_LOCAL`, one the automounter
mounted, or at or below `/net` or `/Network`, apart from an `AfterOnly` matter's own volume at
and below the matter (a different mount inside it stays refused). A folder's entry is read with
`fstatat` from its parent and, on another volume, that volume's flags with `getattrlistat`,
before anything is opened: autofs fires no trigger for a lookup of attributes on the last name
(its `trigger_resolve`), except on a "force a mount" node only its own maps make.

Two backstops cover a stream that loses events without saying so (seen on a disk image mounted
seconds before, under a full parallel test run: nothing for over 10 s). **The canary:** every
10 s the watch rewrites `watch-canary` in the set-aside folder it uses, which is on the matter's
volume, denied to the agent and watched by the same stream (one stream, two paths). Its event
comes about 4 ms after the write on this Mac. When none comes within 5 s, or the canary can't
be written, the stream is lost and the session ends as for a find: kill first, then the final
scan, and the lawyer gets `Denial::GitWatchLost` ("Clauseo stopped the AI tool because it
could no longer watch this matter's folder for changes."). The canary's lateness is judged on
every input, once the canary's events in the batch being read count, and each batch is stamped
when the stream queues it: one read 5 s or more after that ends the session the same way, as
the watch lags behind the agent (an agent that writes nonstop would otherwise hold the canary's
event back behind its own). There is no fallback to rescans. An
event flagged must-scan-subdirectories or dropped (user or kernel) only rescans the matter at
once, and the watch carries on.
**The 30 s pass:** the watch keeps every folder of the matter with its file ID and its
modification and status-change times (from its launch walk, then from its own listings); every
30 s it reads them again and lists only the folders whose ID or times changed, adding their new
subfolders to be listed in turn. A pass stops after 250 ms of work and the next goes on from
there. Either backstop's find ends the session as an event's does.

**Kill first, then set aside.** On the first find while the agent may run, the watch ends every
process of the session and moves nothing until none is left: a loop moving ready repositories in
would otherwise keep one present most of the time. A process group or tree misses a child that
calls `setsid` and outlives its parent (re-parented to launchd), which keeps write access to the
matter. So each launch passes its profile a probe name of its own (`-D
SESSION_PROBE=chat.clauseo.session.<tag>-<n>-<random>`, the random part made once per host
process, so a name of an earlier host run, even one with the same pid, never matches a live
session's), and the profile allows a `mach-lookup` of that
one name, which no service registers. A process belongs to the session when `sandbox_check`
says it is sandboxed, may look up the session's name, and may not look up a control name no
profile allows (`SANDBOX_CHECK_NO_REPORT`, so the checks log no denial). No process can shed
its sandbox, so this finds every process of the session whatever its group or parent
(`proc_listallpids`, about 2 ms for 700 processes). Considered and refused: the responsible
PID (a process can spawn with the "disclaim" attribute and be its own), and a path probe
(`sandbox_check` answers "denied" for every process on a path that doesn't exist). The session
counts as over only when its group is empty and no process carries the probe; its kill is
`TreeKill` plus SIGKILL to each probed process, checked again just before its signal, sent again
every 0.1 s. The kill goes first: the first find of a batch returns at once, before the batch's
other events' scans (a batch's names are all checked first, then its arrived folders on one
budget), a rescan or the 30 s pass, and nothing more is checked while the session ends.
Nothing moves while a process of the session is left: when one outlives the kill's 10 s, the
session's record is marked first, then the final scan only lists what it finds, within the
scan's entry budget (renames by path could be redirected by a live process swapping a folder),
and the record keeps those paths (below). An agent that did not start is treated the same when
a process carrying its probe is left.
The probe fails closed: a `sandbox_check` that errs on a running process keeps the session
running, so only a plain "not sandboxed", "may not look up the session's name" or "may look up
the control name" rules a process out. Such a process is never killed, though: only one every
check plainly puts in the session is, and right before each signal it is checked again and its
start time (`proc_pidinfo`) compared with the listing's, so a reused pid is skipped. A process
whose checks keep failing keeps the session alive past the kill's cap, so its record stays.
A dead session's kill (below) also skips every process that started before the session's
launch time, which its record holds. A process can't shed the probe from inside: a nested
`sandbox-exec` can't apply a profile, and `launchctl submit` hands launchd no job (jail tests).
A watch thread that panics ends the session; an attach to a thread already gone ends it too.
**The watchdog:** a check reads what the agent made while it runs (a folder moved in is walked
whole; a disk can stop answering), and
while one check holds the watch's thread nothing else is checked. So each runner publishes when
its check began and each folder a walk lists, and one watchdog thread per host looks every
0.1 s: a check (one batch of events, a rescan, a 30 s pass) with no progress for 5 s ends the
session by its tree and probe, logged, and the kill goes again on each look while that check
runs; the runner, once its check returns, tells the lawyer (`Denial::GitWatchLost`) and goes to
the final scan. A check still held 10 s after the first kill, with a process of the session
left, has the session's record marked outlived from the watchdog's thread, so the matter's next
launch is refused even if the check never returns. 5 s is the canary's wait: a check held
longer leaves the agent as unwatched as a lost stream. The three windows (the canary, a late
batch, the watchdog) are 5 s each: at 2 s, load averages of 50 and more on the test Mac lagged
the stream or the watch thread past the window and ended sessions that were fine. A rescan of a big matter that keeps
listing folders is not stuck. A session attached after the watchdog gave up on a check is ended
at once.

Then the final scan sets each item aside by one `rename(2)` that never replaces into
`<set-aside>/<UTC time>-<session>-<random>/item`, beside a `record.json` (the original path,
what it was, the time, the session, the outcome, a link's target), and renames the item's own
`HEAD` and `config` there, so the set-aside copy is no working repository. The set-aside
folder is `PolicyBuilder::git_set_aside` (`<data_dir>/sandbox/git-set-aside`): `build()`
refuses one outside every hidden path or meeting the matter or any path allowed again, and the
profile denies it with `file-read* file-write*` as its last file rule. For a matter on another
local volume, a rename can't reach that folder (`EXDEV`), so `build()` names a `.clauseo`
folder at the root of the matter's volume (`Policy::volume_set_aside`), which the profile denies
the same way and the watch never enters; the launch makes it (a real folder owned by the lawyer,
made private where the volume keeps modes) and is refused with a plain reason when it can't (a
read-only volume, a link, another owner), as when no set-aside folder is named. Matters on
other volumes are not refused for being there.

An item that still can't move is neutralised in place: a folder's `HEAD` and `config` get a
random `-set-aside-by-clauseo-<16 hex>` suffix (a `.git` gets one too), a file is renamed the
same way, and a link (which holds no data) is removed once its target is recorded. If that fails
as well, the matter's state file (below) keeps the item, and the matter's launches are refused
("Clauseo won't start an AI tool in this matter until <path> is moved out of it ...") until it
is gone. A folder the final scan can't read is kept the same way, and launches are refused
("... until it can read <path>: the folder could not be read when an earlier AI session ended,
so Clauseo could not check it for Git folders ...") until it can be read. Nothing else is ever
deleted; macOS has no restore of these items.

**The matter's state file**, `<git set-aside>/matters/<hash of the matter>.json` under a
`flock`, records each running session: its host process (pid and start), its probe name, its launch
time, its set-aside folder and its launch baseline, and the matter's path. The session's own watch removes
it after its final scan; when the kill's 10 s ran out with a process left, it marks it instead,
with what that scan found. A session whose
host is gone (or, in this host, whose watch is not running) never ran its final scan, or ran it
with a process left: its surviving processes are killed by its probe (up to 3 s, only those
started since its launch) and, once none is left, its scan runs from its baseline, every folder
counted as old. It moves repository shapes only, never a folder of the lawyer's around them:
each `.git` not the launch's, the `HEAD` of each folder that was not a launch repository (a
repository-shaped one keeps its `objects` and `refs`), each link out and each new `commondir`
path. A repository the lawyer made after the crash loses its `.git` or `HEAD` the same way, and
the notice says where it went. That happens at the **host's start**, for every matter
(`git_watch_start_up`, which
the host calls once on a thread of its own with its git set-aside folder), and at each launch of
the matter, before the watch takes anything in it for the lawyer's. While a process of it is
left nothing of it moves, its record stays, and the matter's launches are refused ("Clauseo
won't start an AI tool in this matter yet: a process of an earlier AI tool session is still
running after Clauseo tried to stop it. Restart the Mac, then try again.", with how many items
it left when the record lists some), until a start or launch finds none left; the record also
stays while the matter can't be found (a disk not plugged in), for the next try. At the **host's quit**, `git_watch_quit(within)` refuses further launches, kills every
running session by its probe (again every 50 ms) and waits up to `within` for the watches to
run their final scans; a watch that doesn't finish keeps its record for the next start.

Every folder the watch leaves alone at launch is one the policy made read-only: the launch's
walk lists every folder holding a `HEAD` (the agent could complete it into a git dir), and the
watch exempts only folders holding a `HEAD`, repository-shaped or not (its `objects`, `refs`
and `commondir` links count wherever they lead, dangling ones too).

**`commondir` targets.** A launch repository's (or `.git`'s) `commondir` that names the matter,
the state folder, a missing path or any folder no read-only rule covers would let the agent
build `objects`, `refs` and `config` there. `build()` lists them: for a missing path the
profile denies writes at and below it; for an existing folder, writes at and below `objects`,
`refs`, `config`, `packed-refs`, `hooks`, `info` and `worktrees` in it (Seatbelt matches
these literals in any letter case on APFS, checked on this Mac). The watch flags anything new at
those paths in the matter.

Each set-aside reaches the launch's sink, uncapped, as `Denial::GitSetAside { path, what,
outcome, session_ended }`. `notice()` is the lawyer's text. The agent runs as the lawyer's
own user, so no one can tell who made the item, and the notice blames no one: "A Git folder
appeared in the matter at Clients/Acme/.git while an AI tool was running. Clauseo stopped the
session and moved it to <path>, because Git folders can run programs when opened. If you put it
there yourself, you can move it back from there." No other move back is offered. Every path a
notice or refusal shows, a stuck item's launch refusal included, has its control characters,
bidirectional controls (U+061C, U+200E, U+200F, U+202A to U+202E, U+2066 to U+2069),
zero-width and invisible characters (U+200B to U+200D, U+2060 to U+2064, U+FEFF) and line
separators shown as escapes (`\u{202e}`), each byte that isn't UTF-8 as `\x80` and a
backslash doubled, so a crafted name can't break the line, reorder the text or pass for
another. `refusal()` is a line for the agent's
log. The policy cost, plainly: agents can't clone or init a repository into the matter, their
temp folder or notes, and a find ends their session. A `.git` or bare repository the lawyer
makes in the matter while a session runs ends that session too and is moved the same way; so
does a big folder the lawyer copies in (past the scan budget), though nothing in it moves
unless it is a repository.

**A matter on a volume the stream can't watch** (no `MNT_LOCAL`, `autofs`, mounted by the
automounter, or under `/net` or `/Network`; `FSEvents` doesn't report other machines' changes).
`build()` reads each volume from an open folder (`fstat` and `fstatfs` on one descriptor, the
nearest existing folder for a path not made yet). It allows such a matter only when no other
path the agent can write (the state folder, the session's temp folder, notes, the Claude store,
the browser socket folder) is on the same volume: then nothing can be renamed in from
elsewhere, and `Policy::matter_watch()` is `MatterWatch::AfterOnly`: the launch runs no live
stream, only the scans before and after the session. Otherwise it refuses with a plain reason.

`CLAUDE.md` at any depth of the matter cannot be created, written, renamed onto or removed, in any
letter case: Claude loads a nested `CLAUDE.md` when it works in that folder. This is stricter than
the brief ("at the matter root"); approved by the orchestrator on 2026-11-02, in line with QC.11
(Rohan may override). The rule is one deny that requires
both `(subpath (param "MATTER"))` and a path-free regex on the last name with two-case letter
classes, `/[cC][lL][aA][uU][dD][eE]\.[mM][dD]$`. It does not fold Unicode normalisation. It
stops writes in place, not a folder moved into the matter that already holds a `CLAUDE.md`: the
agent can make one in its state folder, notes folder or `TMPDIR` (outside the matter, where the
rule does not apply) and move the folder into the matter, or move a folder holding one within the
matter. A nested `CLAUDE.local.md` is writable too: only the root's is protected.

One walk per launch finds links that lead outside the matter; read and write are denied on each,
and on where it leads. Symbolic links that resolve outside (or nowhere) are findings. Hard links
are grouped by (device, inode): a file whose names in the matter equal its link count has every
name in the matter and is not a finding (the lawyer's own linked files stay readable), unless the
group joins a write-denied name (`.git`, a protected name, an over-cap file, a nested `CLAUDE.md`)
to a writable one, when the writable names are findings. Every name of a file with a name outside
is a finding. The walk goes into `.git` folders for hard links only (`git clone --local` hard-links
objects from the repository it copies), never for symbolic links or `.git` files. `build()`
canonicalises every finding's path and target and refuses one outside the matter.

**The link rule budget.** A profile holds at most 256 link rules (`LINK_RULES_MAX`: one per finding
path, and one per distinct symbolic-link target that an allow rule covers: runtime, read, allowed
folders, state, notes, the Claude store, the credential link; a target nothing allows is refused by
`(deny default)` and gets no rule, `Policy::link_targets`). Past it, findings give way to
read-and-write deny rules on folders (`Policy::link_folders`), nearest first: at the deepest level
with a rule that can move, rules are grouped by folder and the largest groups merge into their
folder first (k rules save k - 1) until the count fits; if merging every group of two or more is not
enough, single rules move up a level too and may merge next round; a rule under a folder rule is
dropped. A folder is never the matter root (it holds the protected names), a folder that holds a
`.git` (a working tree), or any folder above one: each would hide the repository and the files
beside it (review round 2). If the count still does not fit, `build()` refuses with `Error::Policy`
naming the count. Measured on this Mac mini (busy): 20,000 hard-link pairs inside the matter give no
findings, build with the walk 0.55 s, jailed `echo` 40 ms; 20,000 hard links to outside files in 100
folders give 100 folder rules, build 0.16 s, jailed `echo` 124 ms.

**Denials.** One `Denial` enum for every OS and the proxy (`Path`, `Host`, `UpstreamAuth`,
`Dropped`, `UpstreamUnavailable`), one `DenialSink` per launch, at most 20 a second, the rest
folded into one `Dropped`.

**The Seatbelt profile** (`src/macos/profile.rs`, `src/macos/base.sb`). Paths reach it only as
`-D NAME=path` parameters read through `(param "NAME")`, numbered for lists (`RUNTIME_0`,
`READ_ONLY_3`, ...); ancestors use `(path-ancestors (param ...))`, so they need no parameters of
their own. The only other values in the text are port numbers and the launch tag, which must be
`clauseo-` and then ASCII letters, digits or `-`. The rule order, where the later matching rule
wins: `(version 1)`; `(deny default (with message "<tag>"))`; the base; read-write on matter and
state; read, run and map on runtime; read on read paths and allowed folders; read-write on notes,
the credential link (literal) and the Claude store's names (literals, lock folders as subpaths);
metadata on the ancestors of every allowed path, and a literal read of `$HOME/.CFUserTextEncoding`
(one parameter: CoreFoundation reads this text-encoding preference at start when
`__CF_USER_TEXT_ENCODING` is not inherited, and a refusal would be a card on every launch); the
Keychain rules when on; write-deny on the read-only paths in the matter, and unlink-deny on their
folders; write-deny on `CLAUDE.md` at any depth; read and write deny on link findings and link
folders; read and write deny on hidden paths; again the state folder, notes, credential link,
Claude store, runtime and read paths, with their ancestors' metadata; unlink- and create-deny on
the matter, state and notes folders themselves (literal: a subpath rule matches the folder too, so
an agent could otherwise remove its folder and leave a link for the next launch); write-deny on
read-only paths in the state folder; the network. Every deny carries the tag. Network: outbound to
`localhost:<port>` for each proxy port and nothing else (no DNS: the proxy resolves names), Unix
sockets in the browser socket folder only, when the host gives one (outbound, and bind for
agent-browser's daemon; amended 2026-11-03, below), and by
`Loopback` bind, inbound and outbound on the listed ports or `localhost:*`.

The base is adapted from Codex's `seatbelt_base_policy.sbpl` and platform defaults (commit 1a604d58,
Apache-2.0, notice kept) but lists system reads instead of Codex's full read access: `/`, `/System`,
`/usr/{lib,bin,sbin,libexec,share}`, `/bin`, `/sbin`, `/Library/Apple`, `/Library/Developer`,
`/private/etc`, `/private/var/db/timezone`, xcode-select's link, `/private/var/select`,
`/Library/Preferences/Logging`, the system's global preferences and the Xcode licence record,
metadata of the root links; the null, random, zero and DTrace helper devices, the process's own
descriptors and the jail's own ptys. No `/dev/tty` (review round 1): run by hand in a terminal, as
the G3, G8b and G10 gates run `run_sandboxed`, a jailed command could otherwise use TIOCSTI to type
into the tester's shell; the app's agents have no controlling terminal, and the jail tests need
none. `/dev/ptmx` and `/dev/ttys*` stay for a pty the jailed process opens itself (`script` runs in
the jail); `/dev/ttys*` requires the kernel's pty extension, which only a pty opened through
`/dev/ptmx` inside the jail carries, and a jail test checks that a pty opened outside the jail
(another terminal of the same user) is refused for read and write. G8b may check it again with a
real terminal. Nothing under `/Users`, `/private/var/folders`, `/private/tmp` or `/Volumes`.

**`SeatbeltLauncher::new(policy, opts, denials)`** implements `process::Launcher`. A launch runs
`/usr/bin/sandbox-exec -p <profile> -D ... <program> <args>` through `spawn_in_tree`, with the
agent's variables plus `TMPDIR` and nothing else, in the same folder. The launcher adds
`SpawnOptions::clear_env` to the options it is given, so no variable of the host's environment
(`HOME`, `PATH`, a token) reaches the jail; R17 passes what the agent needs in the spec. A jail
test runs `/usr/bin/env` and sees only its spec's variable and `TMPDIR`. `sandbox-exec` execs in
place, so the pid, the process group and the tree kill stay `process`'s. Its tag is `clauseo-<host pid>-<counter>`.

**The watcher** (`src/macos/watch.rs`). One `/usr/bin/log stream --style ndjson` per host, with the
predicate `sender == "Sandbox" AND eventMessage CONTAINS "clauseo-"`, started by the first
registered launcher and stopped with the last. The `sender` term keeps out lines a jailed process
writes through `logger`; the tag is the message's last line, which the kernel appends, so a path
holding a newline cannot claim another launch. `file-read-data` maps to a read and every
`file-write-*` to a write, each a `Denial::Path` classed by `Policy::classify`. Metadata and
extended-attribute reads are not reported (runtimes probe many paths that way); network denials are
logged and not reported, since a direct connection is not a site the lawyer can allow; the log's `N
duplicate reports for ...` fold of a reported operation reaches the sink as `Denial::Dropped {
count: N }`; a denial of a device (`/dev/...`) is never reported (every shell asks for `/dev/tty`,
and no device is something the lawyer can allow). A watcher that fails to start or ends is logged
and started again at most once a minute; launches run either way. Each launch's sink sits behind a
`CappedSink`, which the watcher ticks once a second from one interval timer, so lines arriving
faster than once a second do not hold the tick back.

**What R17 must do** (review round 1, items 10 and 11). Build the policy and call `launch` off the
host's actor thread: `build()` walks the matter, and the first launch of a host waits up to 2 s for
`log stream` to start filtering (3 to 6 ms in three runs). The tag, the sink and the watcher
registration belong to the launcher, not to one launch: every launch of a launcher shares them, and
the registration lasts as long as the launcher. So the host makes one `SeatbeltLauncher` per agent
launch and keeps it alive for exactly that launch, dropping it when the agent ends.

## Consequences

- The host must keep a `SeatbeltLauncher` alive while its agent runs, one per launch (above):
  dropping it unregisters the launch, and its later denials are not reported.
- Seatbelt checks a symbolic link at its target, so a credential link must be a hard link (R17);
  a symbolic one leaves the agent unable to read its own login. A hard link's name is checked as
  the name used, so the lawyer's file is still refused under its own path. **Limit:** a hard link
  breaks the first time the agent saves the file by temp file and rename (the rename succeeds in
  the state folder): the lawyer's file keeps a stale refresh token while the state copy rotates.
  The credential link stays a hard link (review round 1, item 6); stage D's split rule, already in
  the plans, handles a rename-save, and G7 row 17 will say how Claude saves its store. Likewise a
  temp-file save of the Claude store, if Claude does one, fails: only its exact names are allowed.
- Seatbelt compared paths without regard to letter case on this Mac's APFS volume: `claude.md`,
  `.CODEX/`, `.GIT/config` and `DATA/...` all met the rules of their canonical names, so the
  profile needs no case-folding rules. The jail tests check the variants. `Policy::classify` folds
  ASCII case on macOS to match, so a refusal of `.GIT/config` is `ReadOnly` and of `DATA/x`
  `Hidden` (NFD and NFC forms of one name are not folded). A nested `.claude/` deeper in the matter
  is not protected (the policy protects the root); a nested `CLAUDE.md` is (above).
- Renaming a folder that holds a protected path would carry it out of its rule (checked on this
  Mac), so the folders above each read-only path in the matter and the state folder cannot be
  unlinked or renamed. The matter folder itself is one of them.
- A link finding whose target holds an allowed path (a link to `..` or to `/`) gets no target rule:
  a subpath deny on it would deny the matter itself. The default deny still refuses it.
- Nesting, probed through the launcher: `sandbox-exec -p '(version 1)(allow default)'
  /usr/bin/true` inside the jail fails with `sandbox_apply: Operation not permitted` (exit 71).
  Codex's own Seatbelt cannot start inside ours; `codex` was not on this Mac's PATH, so `codex
  sandbox macos` was not probed. The profile was not changed for it (G8b decides).
- Under Xcode, `/usr/bin/git` runs through xcrun, which ignores `TMPDIR` for its cache and reads
  Xcode's caches under `/private/var/folders`; those refusals reach the sink (G8b follow-up). git's
  runtime is the Xcode app, or nothing extra with the Command Line Tools.
- CoreFoundation's read of `~/.CFUserTextEncoding` is allowed (above). The policy's home is
  `$HOME`, while CoreFoundation reads the account's home: they differ only when `HOME` is changed.
- Deviation: `process` gains `SpawnOptions::clear_env` (additive; `spawn_in_tree` calls
  `env_clear` before setting the spec's variables). Without it the jail inherited the host's whole
  environment. `PassthroughLauncher` with default options still inherits it.
- Deviation: the jail test cannot use only `fake_agent`, whose file steps are relative writes that
  stop at the first failure. `fake_agent` (scenario `sandbox-probe`) runs the handshake and the
  writes that must succeed; system tools (`cat`, `sh`, `ln`, `nc`, `git`, `logger`) run through the
  same launcher for every refusal and for the G8a cell.
- The Keychain rules are broad until G7 row 17; missing allowances found by G8b become follow-ups.

Amended 2026-11-03 (QD.4): the browser socket folder replaces `<state>/browser/run`. A Unix socket
path is at most 103 bytes on macOS, and the matter's state path is longer than that, so
agent-browser's daemon could not bind there. The host now makes `/tmp/clauseo-ab-<id>`
(`BrowserSocketDir`, Unix: 16 letters of `a-z0-9` from the OS's randomness) with one atomic
`mkdir` at 0700, never reusing a name that exists (a new id is drawn, at most 32 times), and
checks it with `lstat`: a folder, not a link, owned by the effective user (`rustix`'s
`geteuid`), mode exactly 0700; a failure after the `mkdir` removes what it made and fails the
launch. Beside the folder, made first (`O_CREAT|O_EXCL|O_NOFOLLOW`, 0600) and held with `flock`
for the folder's life, sits `/tmp/clauseo-ab-<id>.lock`, outside the folder so the jail cannot
touch it. The lock is taken without blocking right after the open, and the name must then still
point at the file locked (same device and inode, a regular file) before the `mkdir`; a sweep
holding or replacing the new lock file in between costs a new id, not a failed launch. Dropping
the value removes the folder, then the lock file, then releases the lock.
`BrowserSocketDir::sweep_stale` removes, at host start, ours that a crashed host left: real
folders we own with that name shape whose lock file is a regular file of ours that no live
process holds (a non-blocking `flock` takes it, and the name still points at the file locked),
then their lock files, and lone stale lock files the same way; never a link, and never a folder
whose lock file is missing, a link or not ours (a crash cannot leave a folder without its lock
file, which is made first and removed last). A stress test runs four creators against four
sweepers in one folder: no live folder is swept and no create fails.
`PolicyBuilder::browser_socket_dir` puts its canonical path (`/private/tmp/...`) in the policy;
the profile then allows Unix-socket bind and connect, and file write, read and unlink, under that
one folder, and creation there of regular files and sockets only (`vnode-type` filters on
`file-write-create`, which Seatbelt honours on this Mac: `mkdir`, `ln -s` and `mkfifo` are
refused there); it denies creating or removing the folder itself, and allows metadata on the
folders above it. The creation limit is not a guarantee about what the folder holds: on some
macOS versions a swap (`renamex_np` with `RENAME_SWAP`) of a matter path with a file in the
folder can bring a folder in, with whatever it holds, links included. Seatbelt cannot refuse that without refusing
renames the matter needs, and it gives the agent nothing beyond its own write access to the
matter. The removal at drop (`remove_dir_all`) never follows a link; a jail test swaps in a
matter folder holding a link to a file outside the matter (where the jail refuses the swap, the
host plants the same structure), drops the value, and finds the folder gone and the outside file
intact. Nothing else in `/tmp`. With no folder set the jail allows no Unix socket
anywhere. The jail tests check a socket's whole life in the folder, and refusals in `/tmp`, in
another `clauseo-ab-*` folder, in `<state>/browser/run`, of `rmdir`, `rename` and `chmod` on
the folder, of a folder, link or pipe in it, of a link in it to a socket outside, and of a new
folder or file in `/tmp`.

Residual risk: the jail's rules match paths, not owners. A jailed process that outlived its
session could meet a folder another local user made again at the same path after the drop, so
the host drops the value only after the launch's whole process tree has exited. The random
16-letter names, the drop order and agent-browser's daemon living inside the launch narrow it.

Amended 2026-11-03, Unicode normalisation in paths. Hindi content is a hard requirement (private
measurements/hindi-content-audit.md row 3c): a matter named in Devanagari must work, and no deny
rule may be missed because of the form its name is written in. Measured on APFS
(case-insensitive) on 2026-11-03: a folder keeps the form it was made in, but its raw, NFC and
NFD names open the same folder, so on APFS two siblings that differ only in normalisation cannot
exist; `realpath` (behind `dunce::canonicalize`) returns the stored form whichever form it is
given.
- Rust-side comparisons (`policy::under` and `same`, which every policy path comparison goes
  through, `classify` and the link scan included) compare names by one key on macOS: Unicode
  canonical caseless matching, NFD of the full case fold (CaseFolding.txt C and F, not Turkic) of
  NFD, from the `caseless` crate. Case folding is Unicode full folding, not ASCII only. A name
  that is not UTF-8 folds ASCII case only. Elsewhere comparisons are exact. The policy's dedup
  sets (link findings, link targets, read-only rules) use the same keys.
- A test checks the key against this Mac's volume: for each pair a folder is made under one name
  and opened by the other. APFS and the key agree on all 16: the raw, NFC and NFD Devanagari
  name; É and é; Σ, σ and ς; ß and ss; ﬁ and fi; İ and i plus U+0307; K and the Kelvin sign; Å
  and the Angstrom sign; ſ and s; a Hangul syllable and its jamo all match; क and क़, and
  fullwidth Ａ and A, do not.
- Kernel rules use the canonical stored form, with no variants. Seatbelt matches `literal` and
  `subpath` rules across raw, NFC and NFD in every pairing of rule and access form, for existing
  names and for a `literal` on a name not yet made (measured). The jail tests write through every
  form of an NFD-made matter and are refused every form of a hidden folder and a nested
  `CLAUDE.md`.
- NTFS is normalisation-sensitive (an NFC and an NFD name are two files), as are Linux file
  systems, so folding there would merge two folders. S15 uses the path Windows returns for the
  opened handle, with no folding.

Residual risks:
- **A matter on a network volume is watched only before and after a session.** While the agent
  runs only the profile's name denies stop a new `.git` or `HEAD` there; nothing can be renamed
  in, as `build()` refuses the matter when another path the agent writes shares its volume.
  Another machine on the network can still change the matter during the session.
- On FAT32 volumes two siblings can differ only in normalisation (U+0958 and U+0915 U+093C were
  made as two folders on a FAT32 image; on an exFAT image the second form of each of U+0958, ऩ
  and café reported "already exists", as on APFS). A Seatbelt rule that allows one then also
  allows writes into the other, since Seatbelt folds normalisation itself. That kernel behaviour
  predates this change, and the Rust-side folding agrees with it; it matters only for a matter on
  a FAT32 volume.
- `caseless` 0.2.2 folds by Unicode 16; this Mac's APFS folds Unicode 17 too. A check of every
  code point found 56 pairs APFS opens as one folder that the key keeps apart (U+A7CE/CF,
  U+A7D2-D5, and Beria Erfe U+16EA0-16EDx, all new in Unicode 17), and none the other way. For a
  name in those letters the Rust side can label a denial wrongly or accept an overlap the kernel
  still refuses, since Seatbelt applies the rule. Revisit when `caseless` moves to Unicode 17.

## Status

proposed

## Date

2026-11-02
