# Our agent-browser build

The desktop bundles agent-browser as a sidecar (ADR 0029, "Bundling (desktop)"). macOS and
Windows take our build: upstream's source at a pinned commit, plus the patches in `patches/`,
built by our own workflow. Linux keeps upstream's release binary for now: it is used for
development and CI only, and the stream server is still on there. This folder holds the patches;
the pin is `crates/desktop/agent-browser.toml`.

## Upstream

- Repository: https://github.com/vercel-labs/agent-browser (Apache-2.0; our copy of the license
  is `LICENSES/Apache-2.0-agent-browser.txt`).
- Tag: `v0.41.1`, commit `5868f7c05020e047c00e68e4abf9253cce864f41` (a lightweight tag).
- Source: GitHub's archive of that commit,
  https://github.com/vercel-labs/agent-browser/archive/5868f7c05020e047c00e68e4abf9253cce864f41.tar.gz,
  SHA-256 `ca59c4590a5a265d106c494d7726b9cde8575536d31834709dd525da8d778f6f`. It extracts to
  `agent-browser-<commit>/`; the Rust crate is `cli/`, with its own `Cargo.lock`.

The pin holds the same three values (`version`, `upstream_commit`, `upstream_tarball_sha256`) and
`patched_tag`, the name of our release of the build.

## The patches

Each is a `git diff` from upstream's repository root (paths `a/cli/src/...`), applied with
`git apply` inside the extracted folder, in number order. A later fix is a new numbered file, not
an edit to a published one.

`patches/0001-stream-disabled-switch.patch` (every OS; 6 files, +72/-17): upstream always starts
the daemon's stream server, a loopback HTTP server whose API can drive the session. With
`AGENT_BROWSER_STREAM_DISABLED=1` the daemon starts none and writes no `.stream` file,
`stream enable` fails, `inspect` and `dashboard start` refuse to start their servers, and a
daemon started with streaming on is not reused by a CLI that has it off. This is the part offered
upstream; if upstream takes it, macOS can go back to upstream's binary (the pin accepts an
upstream url for any triple).

`patches/0002-windows-pipe-and-hardening.patch` (Windows code paths only; 16 files, +2555/-150,
most of it four new modules and their tests):
- the CLI reaches the daemon over the named pipe `\\.\pipe\clauseo-ab-<128-bit random hex>`
  instead of loopback TCP, with four instances listening at once. `<session>.pipe` in the socket
  folder records the pipe id with the daemon's pid and creation time, and an HMAC of them with
  the session key; the name is never on a command line, and a record without a valid HMAC is
  ignored. The first instance is created with FILE_FLAG_FIRST_PIPE_INSTANCE, its DACL lets only
  the user and SYSTEM in, and the daemon disconnects, before writing a byte, any client whose
  process token is restricted (as a sandbox's is) or cannot be checked;
- the session key: the launcher (Clauseo's browser guard) passes 32 random bytes per session in
  `AGENT_BROWSER_SESSION_KEY`. Every connection starts with a mutual challenge-response: each
  side sends a fresh nonce and proves the key with HMAC-SHA256 over both nonces and the session
  name, checked in constant time, so the key never crosses the pipe. The daemon serves no client
  without the key (an unsandboxed process of the user included), and the CLI talks to no daemon
  of another session or key. Each process takes the key out of its own environment first thing
  and gives it back only to the children it starts hardened (the daemon, the MCP server's CLI
  runs); it is never in a file or a log. Without a valid key the CLI and the daemon refuse to
  run on Windows. HMAC-SHA256 comes from the `hmac` and `sha2` crates already in upstream's
  `Cargo.lock`;
- the CLI also refuses a pipe whose server is not the recorded daemon (same pid and creation
  time, this executable, an unrestricted token), and connects at identification level only;
- a live daemon keeps its files and is never replaced by a second one; the `taskkill` fallback
  is gone (the protected DACL denies PROCESS_TERMINATE anyway);
- every agent-browser process (MCP server, CLI, daemon) sets the protected DACL
  `D:P(A;;0x1FFFFF;;;SY)(A;;0x101000;;;<user SID>)(A;;0x101000;;;OW)` on itself first thing,
  and `D:P(A;;0x1FFFFF;;;SY)(A;;0x100800;;;<user SID>)(A;;0x100800;;;OW)` (no THREAD_RESUME,
  GET_CONTEXT or SET_CONTEXT) on its main thread; the daemon's runtime threads get the thread
  DACL as they start, and the CLI and the daemon are started through CreateProcessW with both
  as their process and first-thread descriptors;
- streaming, `inspect` and the dashboard are always off on Windows.

Any other child agent-browser starts (Chrome, engines, ffmpeg, plugins, probes) starts through
upstream's own code, with agent-browser's environment less the session key, and with the default
process DACL. In Clauseo none of them is reachable on Windows: the guard starts Chrome itself and
gives agent-browser its endpoint (`AGENT_BROWSER_CDP`), so the daemon launches no browser, and
the guard refuses `--engine`.

`patches/0003-version-suffix.patch` (every OS; 2 files): our build reports version
`0.41.1-clauseo.1`, so the CLI's daemon version check tells a patched daemon from an unpatched
one. `agent-browser-build.mjs` refuses the patched source unless its `cli/Cargo.toml` and
`cli/Cargo.lock` both say the version the pin's `patched_tag` names, so a `-clauseo.2` rebuild
cannot report `.1`.

Linux keeps upstream's binary for now: dev and CI only, the stream server still on there.

## Checking that the patches apply

```sh
node crates/desktop/sidecars/agent-browser-build.mjs check
```

It downloads the archive, refuses it unless its SHA-256 is the pin's and it names the pinned
commit, extracts it to a temporary folder, applies each patch after a `git apply --check`, and
removes the folder. It needs node, tar and git, and runs on Linux, macOS or Windows. The script's
tests run with `node --test 'crates/desktop/sidecars/*.test.mjs'`.

## Building and publishing

The workflow `.github/workflows/agent-browser-build.yml`, by hand:

```sh
gh workflow run agent-browser-build.yml --ref <branch>                  # build only
gh workflow run agent-browser-build.yml --ref <branch> -f publish=true  # build and publish
```

Each target builds on its own runner: x86_64 Windows on `windows-2025`, ARM64 Windows on
`windows-11-arm` (native, not the x64 build under emulation), and both macOS triples on
`macos-15` (x86_64 cross-compiled, as release.yml builds the app). Each runs
`node crates/desktop/sidecars/agent-browser-build.mjs build --target <triple> --out <dir>`:
the patched source's own tests (`cargo test --locked`; on Windows these include the pipe's
restricted-token test, the session key tests and the hardening tests), then `cargo build --release --locked` in `cli/`,
with rust-toolchain.toml's toolchain, path remaps and `SOURCE_DATE_EPOCH` set to the commit's
time, plus on Windows `+crt-static` (no VC++ runtime DLL imports; the leg checks with
`.github/scripts/pe-crt-imports.mjs`) and `/Brepro`. Each leg uploads the binary and its build
record, `agent-browser-<triple>.json` (SHA-256, size, `rustc -vV`, the linker's version, the
upstream commit, the archive's and each patch's SHA-256).

With `publish`, the workflow makes the GitHub release `patched_tag` in clauseo-ai/clauseo-acp, a
pre-release that is never marked latest (the updater reads `releases/latest`), with the four
binaries, `SHA256SUMS` and the records. Then paste each triple's `url` and `sha256` lines,
which the build log prints, into the pin's macOS and Windows tables, replacing the all-zero
placeholder the fetch refuses.

The build aims to be reproducible, but a runner image with a newer linker can still change the
bytes. The release trusts the pinned sha256 of the published binary, not a rebuild. Nothing is
signed or notarised here: the release's sidecar fetch puts the binary at
`crates/desktop/binaries/clauseo-agent-browser-<triple>[.exe]`, and release.yml treats it as it
treats the app (ADR 0035, "Signing for the beta"). On macOS the build leg signs
`Contents/Helpers/clauseo-agent-browser` ad hoc (identity `-`, with the hardened runtime), as
the bundler signs the app, until there is a Developer ID; release-publish refuses any DMG that is
not notarised and stapled, so no release goes out before then. On Windows the beta is unsigned:
the SignPath step that will sign every PE in that folder (S20a) is not in release.yml yet.

## What the build trusts

`agent-browser-build.mjs` meets five rules:

1. **Upstream source pinned by commit and verified by hash.** It downloads GitHub's archive of
   `upstream_commit` (a 40-hex commit, never a tag name), refuses it unless its SHA-256 is
   `upstream_tarball_sha256`, and refuses it unless the commit the archive names (its pax
   header) is `upstream_commit`. All three come from the pin in the checkout.
2. **Patches only from the checked-out commit.** It applies exactly the `NNNN-*.patch` files in
   this folder of the checkout it runs from (numbered from 0001, no gaps; any other file but a
   dot file is refused), and `build` refuses to run when `git status --porcelain` shows
   anything uncommitted in this folder. No flag or variable points it elsewhere, and each
   applied patch's SHA-256 goes in the build record.
3. **`--locked`.** Both `cargo test` and `cargo build --release` run with `--locked` against
   upstream's own `Cargo.lock`, which our patches change only for the package's own version
   (0003).
4. **Nothing downloaded without a hash.** The archive is checked as above. Cargo checks every
   crate against the `checksum` in `Cargo.lock` (upstream's lock has no git dependencies). The
   toolchain is our `rust-toolchain.toml` channel, and `build` refuses it unless `rustc -vV`'s
   commit-hash is the pin's `rustc_commit`. The script downloads nothing else.
5. **`tag` needs nothing a publish job would have to trust.** `tag` (and `toolchain`) read
   only committed files (`agent-browser.toml`, `rust-toolchain.toml`) and print one line: no
   network, no child process, no fetched code. A read-only job can run it on the dispatched
   commit and hand the tag to the publish job.

## Bumping

1. Pick upstream's new tag; note its commit (`git ls-remote
   https://github.com/vercel-labs/agent-browser refs/tags/v<version>`, the peeled `^{}` line for
   an annotated tag).
2. Download `archive/<commit>.tar.gz` and take its SHA-256.
3. In the pin, set `version`, `upstream_commit`, `upstream_tarball_sha256`, and `patched_tag`
   to `agent-browser-v<version>-clauseo.1`; point the macOS and Windows urls at the new tag, set
   their sha256 to the placeholder, and update the Linux tables from upstream's release.
4. Rebase the patches on the new source until `agent-browser-build.mjs check` passes.
5. Run the workflow with `publish`, and paste the printed lines into the pin.

A rebuild of the same version (a patch fix, a new toolchain) counts `patched_tag` up
(`-clauseo.2`) and repeats steps 4 and 5: a published release is never replaced.
