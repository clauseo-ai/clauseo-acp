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

`patches/0001-stream-disabled-switch.patch` (every OS; 3 files, +50/-17): upstream always starts
the daemon's stream server, a loopback HTTP server whose API can drive the session. With
`AGENT_BROWSER_STREAM_DISABLED=1` the daemon starts none and writes no `.stream` file,
`stream enable` fails, and a daemon started with streaming on is not reused by a CLI that has it
off. This is the part offered upstream; if upstream takes it, macOS can go back to upstream's
binary (the pin accepts an upstream url for any triple).

`patches/0002-windows-pipe-and-hardening.patch` (Windows code paths only; 15 files, +1541/-141,
most of it three new modules and their tests):
- the CLI reaches the daemon over the named pipe `\\.\pipe\clauseo-ab-<128-bit random hex>`
  instead of loopback TCP. The name is in `<session>.pipe` in the socket folder, never on a
  command line. The pipe's first instance is created with FILE_FLAG_FIRST_PIPE_INSTANCE, its DACL
  lets only the user and SYSTEM in, and the daemon disconnects, before writing a byte, any client
  whose process token is restricted (as a sandbox's is) or cannot be checked. The CLI refuses a
  pipe whose server is not the daemon in `<session>.pid`;
- every agent-browser process (MCP server, CLI, daemon) sets the protected process DACL
  `D:P(A;;0x1FFFFF;;;SY)(A;;0x101000;;;<user SID>)(A;;0x101000;;;OW)` on itself first thing,
  and starts its children (the CLI, the daemon) with it through CreateProcessW's
  lpProcessAttributes;
- streaming is always off on Windows, and the dashboard and the `inspect` proxy refuse to start.

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
