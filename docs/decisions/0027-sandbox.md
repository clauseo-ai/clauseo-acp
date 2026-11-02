# 0027: The sandbox crate, its policy, and Seatbelt on macOS

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

**Read-only names and links.** `.codex/`, `.claude/`, `.agents/`, `.clauseo/`, `.mcp.json`,
`CLAUDE.md` and `AGENTS.md` at the matter root, existing or not; each existing `.git` at any depth
and the folder a `gitdir:` file names inside the matter; over-cap files. One walk per launch finds
links that lead outside the matter (symbolic links, and files with more than one name); read and
write are denied on each, and on where it leads.

**Denials.** One `Denial` enum for every OS and the proxy (`Path`, `Host`, `UpstreamAuth`,
`Dropped`), one `DenialSink` per launch, at most 20 a second, the rest folded into one `Dropped`.

**The Seatbelt profile** (`src/macos/profile.rs`, `src/macos/base.sb`). Paths reach it only as
`-D NAME=path` parameters read through `(param "NAME")`, numbered for lists (`RUNTIME_0`,
`READ_ONLY_3`, ...); ancestors use `(path-ancestors (param ...))`, so they need no parameters of
their own. The only other values in the text are port numbers and the launch tag, which must be
`clauseo-` and then ASCII letters, digits or `-`. The rule order, where the later matching rule
wins: `(version 1)`; `(deny default (with message "<tag>"))`; the base; read-write on matter and
state; read, run and map on runtime; read on read paths and allowed folders; read-write on notes,
the credential link (literal) and the Claude store's names (literals, lock folders as subpaths);
metadata on the ancestors of every allowed path; the Keychain rules when on; write-deny on the
read-only paths in the matter, and unlink-deny on their folders; read and write deny on link
findings; read and write deny on hidden paths; again the state folder, notes, credential link,
Claude store, runtime and read paths, with their ancestors' metadata; write-deny on read-only paths
in the state folder; the network. Every deny carries the tag. Network: outbound to
`localhost:<port>` for each proxy port and nothing else (no DNS: the proxy resolves names), Unix
sockets under `<state>/browser/run` (outbound, and bind for agent-browser's daemon), and by
`Loopback` bind, inbound and outbound on the listed ports or `localhost:*`.

The base is adapted from Codex's `seatbelt_base_policy.sbpl` and platform defaults (commit
1a604d58, Apache-2.0, notice kept) but lists system reads instead of Codex's full read access:
`/`, `/System`, `/usr/{lib,bin,sbin,libexec,share}`, `/bin`, `/sbin`, `/Library/Apple`,
`/Library/Developer`, `/private/etc`, `/private/var/db/timezone`, xcode-select's link,
`/private/var/select`, `/Library/Preferences/Logging`, the system's global preferences and the
Xcode licence record, metadata of the root links; the null, random, zero, tty and DTrace helper
devices, the process's own descriptors and the jail's own ptys. Nothing under `/Users`,
`/private/var/folders`, `/private/tmp` or `/Volumes`.

**`SeatbeltLauncher::new(policy, opts, denials)`** implements `process::Launcher`. A launch runs
`/usr/bin/sandbox-exec -p <profile> -D ... <program> <args>` through `spawn_in_tree`, with the
agent's variables plus `TMPDIR`, in the same folder. `sandbox-exec` execs in place, so the pid,
the process group and the tree kill stay `process`'s. Its tag is `clauseo-<host pid>-<counter>`.

**The watcher** (`src/macos/watch.rs`). One `/usr/bin/log stream --style ndjson` per host, with the
predicate `sender == "Sandbox" AND eventMessage CONTAINS "clauseo-"`, started by the first
registered launcher and stopped with the last. The `sender` term keeps out lines a jailed process
writes through `logger`; the tag is the message's last line, which the kernel appends, so a path
holding a newline cannot claim another launch. `file-read-data` maps to a read and every
`file-write-*` to a write, each a `Denial::Path` classed by `Policy::classify`. Metadata and
extended-attribute reads are not reported (runtimes probe many paths that way); network denials
are logged and not reported, since a direct connection is not a site the lawyer can allow; the
`N duplicate reports` lines are ignored. A watcher that fails to start or ends is logged and
started again at most once a minute; launches run either way. Each launch's sink sits behind a
`CappedSink`, which the watcher ticks once a second.

## Consequences

- The host must keep a `SeatbeltLauncher` alive while its agent runs: dropping it unregisters the
  launch, and its later denials are not reported.
- Seatbelt checks a symbolic link at its target, so a credential link must be a hard link (R17);
  a symbolic one leaves the agent unable to read its own login. A hard link's name is checked as
  the name used, so the lawyer's file is still refused under its own path.
- Seatbelt compared paths without regard to letter case on this Mac's APFS volume: `claude.md`,
  `.CODEX/`, `.GIT/config` and `DATA/...` all met the rules of their canonical names, so the
  profile needs no case-folding rules. The jail tests check the variants. A nested `.claude/` deeper
  in the matter is not protected (the policy protects the root).
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
- CoreFoundation reads `~/.CFUserTextEncoding` at start; the refusal reaches the sink (G8b decides
  whether to allow it).
- Deviation: the jail test cannot use only `fake_agent`, whose file steps are relative writes that
  stop at the first failure. `fake_agent` (scenario `sandbox-probe`) runs the handshake and the
  writes that must succeed; system tools (`cat`, `sh`, `ln`, `nc`, `git`, `logger`) run through the
  same launcher for every refusal and for the G8a cell.
- The Keychain rules are broad until G7 row 17; missing allowances found by G8b become follow-ups.

## Status

proposed

## Date

2026-11-02
