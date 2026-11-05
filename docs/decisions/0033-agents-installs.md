# 0033: `agents`, the Node runtime and silent installs

## Context

A lawyer must get Claude Code's or Codex's ACP adapter with no terminal and no admin rights
(stage D). The adapters are npm packages: claude-agent-acp 0.89.1 and codex-acp 2.2.2 with
`@openai/codex` overridden to 0.162.1. G16's pins file, `crates/limits/tests/live/package.json`
and its `package-lock.json`, is the one place those versions are written. The Claude and Codex
native binaries are npm packages too (`@anthropic-ai/claude-agent-sdk-<platform>`, the
`@openai/codex` platform aliases), so `nodejs.org` and `registry.npmjs.org` are the only install
hosts. `host` cannot depend on this crate (ruling 27): desktop holds both, and the host learns an
installed adapter only as an `agents.<id>` settings entry (R20).

Measured on Linux x64 on 2026-11-02 with Node 24.22.0 and npm 11.19.1:
- A lock holding only the entries reachable through `dependencies` and `optionalDependencies`
  makes `npm ci --offline` fail with `ENOTCACHED` for `@anthropic-ai/sdk`. That is a peer of
  `@anthropic-ai/claude-agent-sdk`, and npm installs peers. With `peerDependencies` followed as
  well, Claude's lock holds 115 of the 136 entries (108 for one platform) and Codex's holds 30
  (25 for one platform).
- `npm cache add <absolute path to a .tgz>` puts the tarball in the cache by its integrity, and
  `npm ci --offline --ignore-scripts --no-bin-links` then installs from the cache alone. It
  leaves the lock unchanged and makes no link. A relative path is read as a GitHub shorthand
  (`user/repo`), so the path is always absolute. (Superseded on 2026-11-04: see below.)
- Measured on 2026-11-04 with npm 11.19.1 and npm 10.9.9: a lock entry whose `resolved` is a
  `file:` URL of an absolute `.tgz` path, with its `integrity` kept, installs under
  `npm ci --offline` with an empty cache, and a file whose bytes changed fails with
  `EINTEGRITY` ("integrity checksum failed when using sha512"). npm encodes the raw text itself
  before parsing it as a URL, so the path is written unencoded; spaces, `#` and `%` work.
- One-platform downloads: Claude 112.3 to 124.9 MB (unpacked 284 to 304 MB), Codex 138.9 to
  167.7 MB (unpacked 353 to 472 MB). Node 24.22.0's archives are 33.8 to 58.3 MB (unpacked 96 to
  202 MB). The table is in `snapshot.rs`.

## Decision

**The crate.** `clauseo-agents` depends on `protocol` (`AgentId`), `process`, `serde`,
`serde_json`, `futures`, `async-channel`, `ureq` (rustls, the platform verifier, blocking, on
the install's own thread), `sha2`, `base64`, `flate2`, `tar`, `zip`, `thiserror` and `dunce`,
with `nix` on Unix and `windows` on Windows. It also depends on `rustls`, the one ureq uses, to
classify TLS errors and for the test proxy's server side. Beyond the brief's list are `rustls`,
`thiserror`, `dunce`, and `nix` and `windows` (the free disk space, and the Windows proxy
setting read from the registry). The brief's `settings` and `async-io` are not normal
dependencies: the caller passes `Locations::data_dir` in, and timers come through
`process::Timer`. Only the example uses `settings` (a dev-dependency). `rcgen` comes only with the
`test-support` feature, for the TLS-inspecting test proxy. It never depends on `sandbox`:
installs run outside it. It has no tokio and no tauri. Idle stop and resume are `host`'s (R20b).

**Agreed deviations from the brief.** Rohan agreed to each of these:
- The derived lock follows `peerDependencies` as well (see Context).
- An install makes three tries, with waits of 2 s and then 8 s. The brief said 2 s, 8 s and 30 s.
- `Blocked` and `DiskFull` stop at once, with no retry.
- TLS trusts the OS's store (the platform verifier) unless the agent has a CA file.

**The pins.** `snapshot.rs` embeds G16's two files with `include_str!`. Each agent gets a
derived pair: a `package.json` with that agent's one root dependency (plus the `@openai/codex`
override for Codex only), and a lock with the root entry and every `packages` entry reachable from
that dependency through `dependencies`, `optionalDependencies` and `peerDependencies`. Entries
keep their paths, versions, `resolved` URLs and integrity hashes. An optional peer that the lock
lacks is skipped. Both files are written as canonical JSON: keys sorted, two-space indent, and a
trailing newline. That way their bytes, and the hash below, do not depend on whether a build
turns on serde_json's `preserve_order`.

**The snapshot.** Each agent has an id, a display name, its root package, the bin path inside it
(`dist/index.js` for both), the hosts its install and sign-in use (for "Ask your IT team"), and a
download size and unpacked size per platform. The sizes are measured from the registry and
stamped with the lock's hash, and a test fails when the lock changes and the table does not.
Node is 24.22.0, with each archive's file name, size, unpacked size and SHA-256 for darwin-arm64,
darwin-x64, win-x64, win-arm64, linux-x64 and linux-arm64. These are copied from `SHASUMS256.txt`
after its signature checked out with the Node release keys (signed by Richard Lau,
`C82F A3AE 1CBE DC6B E46B 9360 C43C EC45 C17A B93C`). There is no live ACP registry fetch. The
snapshot refuses an agent id named `runtime`, `downloads`, `npm-cache` or `signin`.

**The layout** under `Locations::data_dir`:
- `agents/runtime/node-v24.22.0-<os>-<arch>/`: a downloaded Node.
- `agents/<id>/install/<hash>/`: `package.json`, `package-lock.json`, `node_modules/` and
  `clauseo-receipt.json`. `<hash>` is the first 16 hex digits of the SHA-256 over the agent id,
  its derived lock's bytes and the platform, each prefixed by its length. `limits` writes
  `clauseo-claude-usage.mjs` here too, and nothing here touches it.
- `agents/downloads/` (`<sha512 hex>.tgz`, `.part` while partial), `agents/npm-cache/`,
  `agents/npmrc` and `agents/npmrc-global` (empty), and `agents/.lock`.
  `agents/<id>/home/` and `agents/signin/` are S18's.
- No links: npm runs with `--no-bin-links`, archives are extracted without their links, and
  each adapter runs as `node <script>`. After `npm ci`, the temp folder is walked, and a link or
  junction anywhere in it fails the install.

**Node.** Each folder on `PATH` is searched for `node` (`node.exe` on Windows), and then
`%ProgramFiles%\nodejs\node.exe` on Windows. A candidate is resolved through its links
(`dunce::canonicalize`). It is used only if the result is a regular file, `node --version`
(through the launcher, 10 s at most) reports v22 or later, and npm's `npm-cli.js` sits beside it
(`../lib/node_modules/npm/bin/npm-cli.js` on Unix, `node_modules\npm\bin\npm-cli.js` on Windows).
Otherwise the pinned archive is downloaded and checked by SHA-256. On Linux with musl and no
usable Node of the user's, the install fails at once (kind `failed`, no retry). Its log says the
downloaded Node is a glibc build and Node 22 or later must be installed. Musl users bring their
own Node. The archive is read once into memory (at most its snapshot size, about 60 MB) and its
SHA-256 checked on those very bytes, which are the ones unpacked: nothing is opened again by path
after a check, and a mismatch deletes the archive. It is extracted into a temp folder beside
`runtime/<name>`, with the archive's top folder stripped and no link, absolute path or `..`
entry; `node` and `npm-cli.js` are synced to disk, and the folder renamed into place. A runtime
folder is complete only when its own `clauseo-runtime.json` (the archive's SHA-256, synced before
its rename) is there.

**Downloads are Rust's, the install is npm's.** For each package the derived lock lists for this
platform (npm's `os`, `cpu` and `libc` rule; libc on Linux only, musl when the musl loader is
present), the crate fetches `resolved` itself:
- It resumes a `.part` with `Range: bytes=<len>-`. A 200 starts over, and a 206 appends after
  the bytes already there are hashed.
- It hashes as it writes, checks the lock's SHA-512 at the end, and renames to `<hex>.tgz`. A
  mismatch deletes the file.
- Each file has a size cap: Node's archive exactly its snapshot size, a package the agent's whole
  download for the platform (a loose bound; 512 MiB for a snapshot with no size row). A
  `Content-Length` (plus the resumed bytes) over the cap is refused before anything is written,
  and a body that runs past it is cut off. Either way the `.part` is deleted and the try fails.
- A body that starts with `<` or comes as `text/html`, where a gzip or zip file should be, is an
  HTML page.
- At most 3 redirects. Connect timeout 30 s. The response headers, and each read of the body,
  may take 60 s at most (ureq's `timeout_recv_response` and `timeout_per_read`): a stall timer.
  The body as a whole has no time limit, since a slow link may need minutes.
- TLS uses rustls with the OS's trust store (the platform verifier), so a CA that IT installed is
  trusted. With an agent's `network.ca_certs` file, that file's certificates are the roots.
- Every request and redirect must be https (ureq's `https_only`). An `http://` mirror is refused
  before any request, with a detail naming the mirror setting. Only the `test-support` feature's
  `InstallConfig::allow_http_for_tests` lets the tests' plain-HTTP loopback registry through.

The crate then writes the derived `package.json` and lock into `install/.tmp-<hash>-<n>`, with
each entry's `resolved` replaced by a `file:` URL of its checked tarball (in `agents/downloads/`,
or in the offline bundle), and runs `node <npm-cli.js> ci` there. Every other byte of the lock,
`integrity` included, is the snapshot's, so `<hash>` and the receipts do not change. The URL is
`file://` and the absolute path with a leading `/`: `file:///home/u/…`, and on Windows
`file:///C:/Users/u/…` (backslashes turned to `/`, a `\\?\` prefix dropped), the form
npm-package-arg reads back as the same absolute path on every OS. npm checks each tarball it
reads against the entry's `integrity`, on top of the download's own check. Entries for other
platforms keep their registry `resolved`; they are inert, since npm skips them and `--offline`
would refuse to fetch them. An entry npm installs on this platform without a tarball stops the
try (npm would skip a missing optional one and still succeed), as a failure no retry can help.
On Windows a data folder on a share (`\\server\share\…`) is refused before any download, with
no retry: npm reads its tarballs' `file:` URLs from the wrong path (see firm IT mode). The run
gets `--cache
<npm-cache> --userconfig <agents/npmrc> --globalconfig <agents/npmrc-global>
--update-notifier=false --logs-max=0`, both files empty (npm refuses one file for both), so no
npmrc of the user's or the machine's applies. `ci` adds `--offline --ignore-scripts
--no-bin-links --no-audit --no-fund --allow-file=all`, so it makes no network call;
`--allow-file=all` overrides an `npm_config_allow_file` in the environment the run inherits,
which npm 11 would otherwise apply to the `file:` tarballs (npm 10 accepts the flag). npm still
copies each tarball it reads into the private cache, so the cache folder stays. No proxy and no
CA file reach npm (no `--proxy`, `--https-proxy`, `--cafile` or
`NODE_EXTRA_CA_CERTS`): npm never uses the network here, and a proxy's password on a command line is
visible to other local users. Each npm run gets `NODE_OPTIONS` set empty. Each run is raced against
a pause and a 10-minute timeout. Only npm's `npm error code X` line reaches the log, redacted. An
`ENOSPC` code is `DiskFull`. The temp folder is renamed to `<hash>`, and the receipt is written
last. npm runs as `node npm-cli.js`, never `npm.cmd`, which `LaunchSpec::new` refuses. Every child
starts through the `process::Launcher` the crate is given. Just before `Ready` is sent, the
install's tarballs, the Node archive and the npm cache are removed.

**The receipt** (`clauseo-receipt.json`) records schema 1, the agent id, the adapter version,
the lock hash, the Node path and version, the platform, and `installed_at` in Unix seconds. A
folder without a valid receipt is incomplete and is removed. So are temp folders and other
`<hash>` folders of the same agent (old pins), at the next `Agents::start`, since nothing runs
from them then. At start a receipt's Node is run only when it is the runtime under
`agents/runtime/` (complete, and answering `--version` through the launcher) or the very program
the `PATH` search finds now. A receipt naming any other Node (an nvm switch, say) is rewritten
atomically to name the Node found now, else the usable runtime, and the agent stays ready; only
with no usable Node is the install redone. A `--version` that times out or will not start is not
proof of a bad Node: the agent stays ready and is checked again at the next install or start.
Only a clear bad answer (a non-zero exit, or a version below 22) fetches the runtime again. If
npm cannot start from the runtime, or cannot load its own modules, the runtime folder is removed
so the next try fetches it. The data folder at rest is
trusted (anyone who can write it can edit the adapter after a genuine install), so receipts mark
completeness, not content.

**Progress** is bytes received and the total for the agent, with Node's archive inside the total
and never named. The total adds Node's archive size from the snapshot, each package's size once
its response's `Content-Length` arrives (or the file is already here), and the snapshot's size
row standing in for the packages not sized yet. A `Downloading` event goes out at most every
250 ms on the clock's monotonic reading, and the first and the completing events always go out.
States go out on an `async-channel`. `Missing` carries the bytes an install would download.

**Failures.** One install makes up to three tries:
- `Offline`: name resolution fails, or connect fails with no route (network or host unreachable,
  network down, connect timeout). The install waits 30 s on the timer and tries again, with no try
  counted, until it succeeds or is paused.
- `Blocked`: a 403 or 407, a TLS failure, or an HTML page. It stops at once with no retry, since
  a retry would meet the same proxy.
- `DiskFull`: the free space at the data folder is below the need before the first try (checked
  again before each retry), or `ENOSPC` while writing. It stops at once with no retry. It carries
  the need and the free space in MB.
- `Failed`: anything else, after three tries, with back-off 2 s and then 8 s. That includes a
  third integrity mismatch and a non-zero `npm ci`.

A download reports a `FetchKind`, internal to the crate: `Offline`, `Blocked`, `DiskFull`,
`Integrity`, `Cancelled` or `Failed`. The public `FailureKind` has four: `Offline`, `Blocked`,
`DiskFull` and `Failed`. `Integrity` counts as a try and becomes `Failed` after the third.
`Cancelled` ends quietly as `Paused`.

Each failure carries an `InstallLog`: "Clauseo download log", "agent: {id}", "received: {n} of
{total} MB", "result: {kind} after {n} tries", the failing host and HTTP status, and redacted
details. No npm output goes in raw. User names and passwords are stripped from every URL, the
data folder shows as `<data>`, and any other absolute path as `<path>`.

**The disk need** is twice the download (downloads plus the npm cache) plus the unpacked size,
plus Node's archive and unpacked size when Node is downloaded, plus 10 %.

**One download at a time.** `install` for one agent pauses any other agent's running install
where it is (`Paused`, which keeps its partial files). It resumes on its own `install`. A second
`install` of an agent already installing starts nothing. Jobs run one after another: a new job
starts its work only after a paused one has stopped. There is one worker thread plus a thread per
install, and a job lock makes the jobs run one after another. Every job event is sent under the
lock `pause` takes, and dropped once the job is paused or superseded.

Between processes, each job holds an exclusive lock on `agents/.lock` (`File::try_lock`) from
its start until just before its last event. A lock another process holds means another Clauseo
is installing: the job fails at once with a log line saying so, and a retry works once that one
is done (the simpler choice; the single-instance plugin makes it rare). The start-up clean-up of
stale install folders runs only with the lock held, and is skipped without it. The data folder
and the bundle must be absolute paths; a relative one fails the install with a line naming the
setting, and start-up neither cleans nor reads anything under a relative data folder.

**Firm IT mode.**
1. An id in `managed` (policy sets `agents.<id>`) is IT's runtime. Nothing is installed for it
   and it reports `Managed`.
2. An offline bundle folder (`install.bundle`, R20's setting) holds `node/<Node's archive name>`
   and `npm/<sha512 hex>.tgz`. It is read instead of the network, with the same checks, and a
   missing file fails the install. Node's archive is copied into `agents/downloads/` as it is
   hashed (the download's own write path, with its size cap), and only the copy is unpacked, so a
   share that serves other bytes to a second read cannot pick the Node that runs. An npm tarball
   is checked where it sits; npm reads it again through its `file:` URL and checks it against the
   lock's `integrity`, so a swapped file is `EINTEGRITY`. On Windows a bundle on a share (a
   `\\server\share\…` or `\\?\UNC\…` path) is the exception: npm 11 turns
   `file:////server/share/a.tgz` into `file:\\server\share\a.tgz` when it loads the lock, and
   its fetcher then reads that as `C:\server\share\a.tgz` (checked by running npm's own
   npm-package-arg and arborist code as win32). So each such tarball is copied into
   `agents/downloads/` as it is hashed, as Node's archive is, and npm reads the copy. Making a
   bundle is S25's.
3. A firm npm mirror (`install.registry`, R20's setting): `mirror_url` replaces each `resolved`
   URL's snapshot registry base (`https://registry.npmjs.org/` in the build) with the mirror's
   base. A URL off that base is refused. The integrity check is unchanged.
4. Proxy and CA: the proxy is R17's `network.upstream_proxy` or `HTTPS_PROXY` (the caller picks
   which), and the CA is R17's `network.ca_certs` entry for the agent. `retry(agent,
   use_system_proxy: true)` adds the OS's static proxy setting (Windows' Internet Settings,
   macOS's `scutil --proxy`; Linux has none) and keeps it for later installs. PAC files and
   authenticating proxies wait for G10.

## Consequences

- A pin bump in G16's file moves both agents to new `<hash>` folders by itself. The sizes table
  must be measured again, and its test says so.
- `limits`' path rules hold for the layout. A test in `crates/limits/tests/` points a settings
  entry at a fixture install laid out by this crate.
- R20 adds the four codes to `ErrorCode`, wires `Agents` into desktop, and writes `Installed` as
  the `agents.<id>` entry.
- An install is not a hot path. Nothing runs once installs are done.
- Release builds must build `clauseo-desktop` alone (`-p clauseo-desktop`, as `tauri build`
  does): a combined build with this crate's tests would merge in `test-support`, which allows
  plain-HTTP downloads.
- Accepted from the security review (S17), left as they are:
  - A download has no overall time limit, only the 60 s stall timer: its bytes are capped, and a
    pause stops it.
  - The user's own Node is not checked against a digest (by design); the remaining risk is a
    `PATH` folder that other accounts can write to.
  - Windows reserved names (`CON`, `NUL`, `COM1`), trailing dots and size limits are not checked
    when unpacking: the archive is pinned by SHA-256, checked on the bytes unpacked.
  - A glibc Linux with musl also installed reads as musl, so the install fails and asks for the
    user's own Node.

- 2026-11-04: `npm cache add` was dropped. It was the slowest step of a first install on
  Windows (44.1 s of 79.8 s for Claude with Defender on), and a `file:` `resolved` with the
  lock's `integrity` lets `npm ci --offline` read the checked tarballs in place with the same
  integrity check, proved by an install test that swaps a tarball after the download's check and
  sees `EINTEGRITY`.

## Status

proposed

## Date

2026-11-02
