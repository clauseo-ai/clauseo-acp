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

What the name rules cannot see, covered by the lawyer-side watch (a follow-up): a folder that
already holds a `.git` or a bare layout, made in `TMPDIR`, the state or notes folder, and moved
into the matter (checked on this Mac, kept as the jail test
`a_folder_holding_a_git_moved_in_is_not_stopped_by_the_name_rule`); a directory clonefile
(`cp -c -R`) of such a folder into the matter; a symbolic link in the matter to a `.git` outside
it.

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
read-only paths in the state folder; the network. Every deny carries the tag. Network: TCP over
IPv4 out to `localhost:<port>` for each proxy port and nothing else (no DNS: the proxy resolves
names), Unix sockets in the browser socket folder only, when the host gives one (outbound, and
bind for agent-browser's daemon; amended 2026-11-03, below), and by `Loopback` TCP out over IPv4
and IPv6 to the listed ports or `localhost:*`. No inet bind or listener, whatever the options,
and a trailing `(deny network-bind network-inbound (local ip "*:*"))` among the last rules
(amended 2026-11-09, J3: before, `Loopback` also allowed bind and inbound, as `ip` rules that
passed UDP too).

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

Amended 2026-11-09: the browser profile (J3, ruling a1). macOS refuses a nested `sandbox_init`
in a process that already runs under a Seatbelt profile with any deny, so Chrome's own sandbox
cannot start inside the agent's jail, and `--no-sandbox` inside the jail would let a renderer
exploit reach the matter unseen. So the browser stack (the guard, agent-browser and Chrome) runs
outside the jail, started by the host, under its own profile (`macos::browser::profile`, applied
by `BrowserLauncher` through `sandbox-exec`), which stands in for Chrome's sandbox.
`--no-sandbox` is allowed there and nowhere else. The browser is chrome-headless-shell only: an
app-bundle Chrome needs LaunchServices (`launchservicesd`, `lsd.mapdb`), `configd`,
`coreservicesd` and the window server (each measured: a crash or a hang without it), and the
profile refuses a Chrome path with a `.app` name (`BrowserProfileError::AppBundle`).

`BrowserPaths` names the run folder (`<data>/browser-runs/<run>/` with `profile/`, `staging/`,
`sock/`, `ab/` and `tmp/`), the headless shell's folder, the stack's binaries, the proxy's browser
port and the host bridge's DevTools port, and the deny roots (the matters, the home's sensitive
folders). It refuses relative paths, a run folder not directly in `browser-runs`, a run folder that
equals, holds or sits in a deny root, a binary in a deny root or in the run folder, a path holding a
quote, backslash or control character (paths are written into the text as strings), and port 0.
`BrowserLauncher` refuses to start when this process is already sandboxed (`sandbox_check` with no
operation is not 0: `AlreadySandboxed`), clears the environment, and sets `HOME` and `TMPDIR` to
`tmp/` and `MAC_CHROMIUM_TMPDIR` to `ab/` (Chrome reads its temp folder from that variable, not
`TMPDIR`, and binds its singleton socket there; without it Chrome uses `/var/folders` and exits).
The flags: `--headless --no-sandbox --disable-gpu --user-data-dir=<run>/profile
--proxy-server=http://127.0.0.1:<proxy>`. Chrome starts through `BrowserLauncher::launch_chrome`,
which puts `--remote-debugging-pipe` first, drops any `remote-debugging-port` or
`remote-debugging-address` switch, and gives Chrome a fresh pipe pair at fds 3 (commands in) and 4
(replies and events out), each message one JSON text ended by a NUL (`macos::browser::pipe`).
`sandbox-exec` passes fds 3 and 4 through (measured), and the fds reach the child by
`SpawnOptions::pass_fd` in `clauseo-process`. The caller gets its ends as a `CdpPipe` in a
`ChromeLaunch`. macOS has no `pipe2`: a pipe is made, then marked close-on-exec in a second call,
and a child forked in between keeps both ends. So the pipes come from
`clauseo_process::cloexec_pipe`, made under the lock `spawn_in_tree` holds while it starts any
child. A start outside that crate takes no such lock, so both sandbox launchers (the agent's and the
browser's) also set `SpawnOptions::close_other_fds`: before exec the child closes every descriptor
of 3 or above it was not passed and that is not close-on-exec. It lists them with
`proc_pidinfo(PROC_PIDLISTFDS)` into a stack buffer, or tries each up to `getdtablesize` when the
list does not fit. Tests: a descriptor left open across exec reaches a plain child but not a jailed
agent or the browser stack, and no plain child started while Chrome's pipes were being made held one
of them. In the test runs, cargo's jobserver pipe (fds 10 and 11) reached every plain child: such
inherited descriptors are what the option closes. The pipe's caller is stage E's host bridge (J4):
it runs outside every jail, launches Chrome here and serves the pipe on `127.0.0.1:<bridge_port>`,
the one DevTools port agent-browser, inside this profile, connects to. The headless shell renders a
local page and screenshots it driven over the pipe alone (measured, chrome-headless-shell 155).
agent-browser's stream (`AGENT_BROWSER_STREAM_PORT`) is dropped: its daemon tries
`127.0.0.1:<port>`, then `127.0.0.1:0`, logs "Stream server failed to start: ... Operation not
permitted" and carries on, its Unix socket in `ab/` made (measured with agent-browser 0.39.1).

What Seatbelt checks on a socket (measured on macOS 27.2, `sandbox-exec` probes run against
clients on loopback, the LAN address and the Tailscale address):
- `network-bind` is checked at `bind()` and `network-inbound` at `listen()`, both against the
  socket's local address only. `accept()` and `recv()` are not checked: no rule on the remote
  address (`(remote ip ...)`, `(remote tcp ...)`, deny then allow) refused a connection or a
  datagram once the listener existed.
- `localhost`, the only host besides `*` a filter takes, matches any local address: a rule
  `(local ip "localhost:P")` let a listener bind `0.0.0.0:P`, `[::]:P` and the LAN address on
  P, and every peer that reached P was accepted. A bind or inbound rule is a port filter. Outbound
  `localhost` is the same: any address of this Mac, though nothing off it.
- `ip` forms pass UDP; `tcp4` refused a UDP bind.
So no rule holds a jailed listener to loopback. Neither profile allows an inet bind or listener
at all: the browser profile allows binds of Unix sockets only, the agent's profile likewise
(agent-browser's socket folder), and only outbound TCP otherwise.

The profile is `(deny default)`, an explicit `(deny file-map-executable)`, then allows only:
- run, read and map of the headless shell's folder and the stack's binaries; read and map of
  `/System` and `/usr/lib` (frameworks, dylibs, fonts: a trap or abort without either), read of
  `/` (dyld aborts without it), metadata of `/etc`, `/tmp`, `/var` (not needed; about 2,600
  denials a run without it) and of each folder above the allowed paths, as literals, so no
  entries are listed (a trap without them; the guard `lstat`s each from `/` down to `sock/`);
- `process-fork` (a trap without it); signals and process info within the sandbox (not needed
  for a run; kept for teardown). `sandbox_check(pid, NULL, 0)` needs no rule: from inside the
  profile it answers 1 for Chrome's browser, GPU, utility and renderer processes (measured);
- read and write of the run folder, the only writable path; `/dev/null` (a trap without it);
- the sysctls `hw.machine`, `hw.memsize`, `hw.ncpu`, `hw.pagesize_compat`, `kern.hostname`,
  `kern.osproductversion`, `kern.osrelease`, `kern.ostype`, `kern.version` (a trap or abort
  without any one); `RootDomainUserClient` (a SIGSEGV without it);
- `mach-register` and `mach-lookup` of `org.chromium.Chromium.MachPortRendezvousServer.<pid>`
  only, Chrome's port broker (`bootstrap_check_in ... Permission denied` without the register;
  the helpers hang without the lookup). No other Mach service;
- Unix sockets: bind and connect at `sock/guard.sock` and under `ab/` (agent-browser's daemon,
  Chrome's singleton); TCP over IPv4 (`tcp4 "localhost:<port>"`) out to the proxy's and the
  bridge's ports only. No inet bind or listener, no UDP, no IPv6, no other port.
The last rules are denies, with no allow after them: no run or map of anything in the run
folder (the agent writes `staging/`), no keychain (folders and services, as in the jail), no GPU
or IOSurface user client (Chrome draws with SwiftShader), no window server or `CARenderServer`,
no `nvram`, no `iokit-get-properties`, no inet bind or inbound on any address or port
(`(local ip "*:*")`), and no `process-info*` on a process outside the sandbox
(#811's rule; `(deny default)` does not refuse it, so a renderer exploit could read every
process's arguments), with `process-info-listpids` allowed back right after, numbers only, as
#811 does: the guard finds Chrome's children by listing pids and reading each one's parent
(`proc_pidinfo`), which reads only for the sandbox's own processes. The jail tests run under it:
matters, the home, `.ssh` and other state refused for read and write; the keychain refused; the
window server, `lsd.mapdb`, `configd` and an unknown name refused, the port broker's name passed; an
outside process's arguments and info refused, the pid list and a sibling's arguments and parent
read; only the proxy's and bridge's ports reachable; every inet bind refused with EPERM (TCP and
UDP; `127.0.0.1`, `0.0.0.0`, `[::]` and `[::1]`; port 0 and fixed ports); a Unix listener in `ab/`
binds, accepts and connects; the ancestors `lstat` but do not list; a binary or library dropped in
the run folder neither runs nor maps; no socket bound in `sock/` but `guard.sock`; the outermost
refusal; Chrome's pipe at fds 3 and 4 with the pipe flag and no port flag (a stand-in); the headless
shell's render and screenshot of a local page over the pipe (skipped when none is on the machine);
and agent-browser's daemon up without its stream server (skipped when it is not installed). Unit
tests pin the pipe framing, the arguments, and that no allow of either profile, under any
combination of the builder's options, opens an inet bind or listener.

The guard's marker (D3): `sandbox_check(pid, "file-write-data", SANDBOX_FILTER_PATH |
SANDBOX_CHECK_NO_REPORT, path)` answers 0 for a write under `<data>/browser-runs/` only under this
profile (the jail hides the data dir: 1), and `file-read-data` of the home answers 1 in it (0
unsandboxed). The kernel answers 1 for a path that does not exist even with no sandbox, so the
guard checks paths that exist. `macos::browser::{sandboxed, pid_sandboxed, path_allowed}` wrap
the call; the guard mirrors them rather than depending on the crate, so their meaning is fixed.

The agent's side (`PolicyBuilder::browser`, `BrowserBridge { socket, staging, bundles,
write_denied, ports }`): a connect to exactly `<run>/sock/guard.sock` (a `path-literal` rule; no
bind, create or unlink in `sock/`), read of the run's `staging/`, and as last rules: no run,
read, map or write of each bundle (`Contents/Helpers`, `<data>/browsers`, so the browser
profile's shell and binaries; a new helper there is refused by its folder's rule), no write of
each write-denied folder (`<data>/browser-pins`: the signed list, the install record), no
write, run or map anywhere in `<data>/browser-runs/` and no read there but the run's staging, its
ancestors and the socket (`require-not`, so the exceptions survive with no allow after the
denies). The builder refuses, while a bridge is set: `Loopback::Any`, a loopback or proxy port
that is the bridge's DevTools port, a browser socket folder, a runtime path in a
bundle, and a bundle or write-denied folder that meets a path the agent writes (planted code
there would run outside the jail). With a bridge or without, no run or map of an installed browser
(`INSTALLED_BROWSERS`: Chrome and its channels, Chromium, Edge and its channels, Brave, Vivaldi,
Opera, Arc) in `/Applications` or the home's `Applications`, matched by a case-folded regex on
`<name>.app/`, since Seatbelt cannot match bundle ids; read stays. The agent profile never allows
`mach-register`, nor any Chromium Mach name.

Residual risks:
- A Chromium the agent copies into the matter (from an installed browser, whose read stays): the
  matter allows running it. A headless shell copied there dies at start in the jail, because
  `bootstrap_check_in` of its port broker is refused (T9, measured with `native_code_deny` off
  and on). A Chromium modified to run without its rendezvous server would get further; closing
  that is `native_code_deny`'s, once on.
- The browser profile allows `mach-register` of rendezvous names for any pid. A compromised
  renderer could register the names of pids not yet in use and so stop a Chromium outside the
  sandbox from starting: denial of service only, accepted.
- Chrome shares its sandbox with the guard and agent-browser, so a compromised Chrome could
  replace `guard.sock` and answer the agent itself: a compromised browser feeding the agent,
  which it can do through the guard anyway.
- The profile lets the stack write its whole run folder, downloads included, not only
  `staging/`: the bridge reads only from `staging/`, so nothing else reaches the agent, and no
  downloads-only rule is needed.
- The bridge's DevTools port is reached by `tcp4 "localhost:<port>"`, which matches any address
  of this Mac: a listener another program holds on that port on the LAN address would be reached
  too. Outbound only, and only to this Mac; the bridge binds `127.0.0.1` explicitly.
- agent-browser's stream (screencast and input) is gone with its listener; nothing in the repo
  read it.
- Seatbelt's regex: `#"^[A-Za-z0-9.-]+\.MachPortRendezvousServer\.[0-9]+$"` as a `global-name-regex`
  matched every Mach service on this Mac (logd, SecurityServer). The rule uses the exact name.

## Status

proposed

## Date

2026-11-02
