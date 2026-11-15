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

`patches/0002-windows-pipe-and-hardening.patch` (Windows code paths only; 30 files, +2029/-197,
most of it three new modules and their tests):
- the CLI reaches the daemon over the named pipe `\\.\pipe\clauseo-ab-<128-bit random hex>`
  instead of loopback TCP, with four instances listening at once. `<session>.pipe` in the socket
  folder records the pipe id with the daemon's pid and creation time; the name is never on a
  command line. The first instance is created with FILE_FLAG_FIRST_PIPE_INSTANCE, its DACL lets
  only the user and SYSTEM in, and the daemon disconnects, before writing a byte, any client
  whose process token is restricted (as a sandbox's is) or cannot be checked;
- the CLI refuses a pipe whose server is not the recorded daemon (same pid and creation time,
  this executable, an unrestricted token), and connects at identification level only;
- a live daemon keeps its files and is never replaced by a second one; the `taskkill` fallback
  is gone (the protected DACL denies PROCESS_TERMINATE anyway);
- every agent-browser process (MCP server, CLI, daemon) sets the protected DACL
  `D:P(A;;0x1FFFFF;;;SY)(A;;0x101000;;;<user SID>)(A;;0x101000;;;OW)` on itself and its main
  thread first thing, the daemon's runtime threads get it as they start, and the CLI and the
  daemon are started with it as their process and first-thread descriptor through
  CreateProcessW;
- every other child agent-browser starts on Windows (Chrome, engines, ffmpeg, plugins, probes)
  gets only the base Windows environment variables, so the CDP url stays out of unprotected
  memory;
- streaming, `inspect` and the dashboard are always off on Windows.

`patches/0003-version-suffix.patch` (every OS; 2 files): our build reports version
`0.41.1-clauseo.1`, so the CLI's daemon version check tells a patched daemon from an unpatched
one.

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

`.github/workflows/agent-browser-build.yml`, by hand:

```sh
gh workflow run agent-browser-build.yml --ref <branch>                  # build only
gh workflow run agent-browser-build.yml --ref <branch> -f publish=true  # build and publish
```

Each target builds on its own runner: x86_64 Windows on `windows-2025`, ARM64 Windows on
`windows-11-arm` (native, not the x64 build under emulation), and both macOS triples on
`macos-15` (x86_64 cross-compiled, as release.yml builds the app). Each runs
`node crates/desktop/sidecars/agent-browser-build.mjs build --target <triple> --out <dir>`:
the patched source's own tests (`cargo test --locked`; on Windows these include the pipe's
restricted-token test and the hardening tests), then `cargo build --release --locked` in `cli/`,
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
bytes. The release trusts the pinned sha256 of the published binary, not a rebuild. Signing
needs nothing here, and nothing is notarised here: the release's sidecar fetch puts the binary in
`crates/desktop/binaries/` as `clauseo-agent-browser-<triple>[.exe]`, where release.yml signs
it as today. On Windows `sign-windows` signs every PE there; on macOS the build leg codesigns
`Contents/Helpers/clauseo-agent-browser` with the app's identity and the hardened runtime
before bundling, and the app is notarised as a whole.

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
