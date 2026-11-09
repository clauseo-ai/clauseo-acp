# Clauseo's Python runtime

Clauseo ships its own Python so that document tools (Word, Excel, PowerPoint, PDF) behave the same
on every machine and never depend on what the user has installed. This folder pins that runtime:
CPython from [python-build-standalone](https://github.com/astral-sh/python-build-standalone) (PBS)
plus a fixed set of document libraries, and [pandoc](https://pandoc.org) from its official release
archives, each file pinned by sha256.

The archives are built per target by xtask's `python-runtime build` and published as GitHub
releases by [.github/workflows/python-runtime.yml](../.github/workflows/python-runtime.yml). The app
does not download them yet; that, and the overlay (Clauseo's `sitecustomize.py`, pip wrapper and
launcher), come in later changes.

## What is here

| path | what it is | edited by |
|---|---|---|
| `lock.toml` | every input: the CPython and PBS release, each target's PBS archive, each package's wheel per target, and pandoc's release archive per target and its source tarball, with sha256 and size | `bump` (only `[runtime]`, `[resolve]` and `[pandoc] version` by hand) |
| `THIRD-PARTY-NOTICES.txt` | every licence text the archives ship, stamped with the lock's sha256; `build` copies it into each archive | `bump` |
| `smoke/smoke.py`, `smoke/hello.pdf` | the smoke test `build` runs with the finished runtime | hand |
| `tools/pack.py` | packs the runtime into a deterministic `.tar.gz`, run by the runtime itself | hand |

## Targets

| target | archive | built on |
|---|---|---|
| `aarch64-apple-darwin` | `clauseo-python-<python>-c<build>-macos-arm64.tar.gz` | `macos-15` |
| `x86_64-apple-darwin` | `clauseo-python-<python>-c<build>-macos-x64.tar.gz` | `macos-15-intel` |
| `aarch64-pc-windows-msvc` | `clauseo-python-<python>-c<build>-windows-arm64.tar.gz` | `windows-11-arm` |
| `x86_64-pc-windows-msvc` | `clauseo-python-<python>-c<build>-windows-x64.tar.gz` | `windows-2025` |

Each archive holds one folder, `python-<python>-c<build>-<os>-<arch>/`, with no symlinks (Clauseo's
extractor skips links; macOS's `python` and `python3` are copies of `python3.13`). macOS wheels must
run on macOS 13.0, the app's floor.

pandoc is one file in each archive: `bin/pandoc` on macOS, `clauseo-bin\pandoc.exe` on Windows
(both Windows archives take pandoc's x86_64 build; Windows on arm64 runs it under Prism). Only the
agent's environment puts that folder on PATH; nothing else changes PATH. It comes from jgm/pandoc's
GitHub release, which publishes no build provenance or signature files, but is an immutable release
with GitHub's signed release attestation: `bump` takes each archive's sha256 from GitHub's API,
and `build` checks the sha256 and size, then `gh release verify-asset <version> <file> -R
jgm/pandoc`. pandoc is GPL-2.0-or-later and runs as its own program; the notices carry its
`COPYING.md` and `COPYRIGHT` (from the tag's source tarball, which the lock pins too) and point at
that tarball as its complete source. `c<build>` is Clauseo's build number: any change to what goes
in an archive takes a new one, and a published build is never replaced.

## Commands

All run from the repo root.

- `cargo xtask python-runtime check`: offline. The lock parses and is laid out as `bump` writes
  it; it has all four targets; every package is a wheel (no sdists) whose tags load in CPython
  3.13 on its target; a package with compiled code has a wheel for every target; every licence is
  in the allowlist (permissive licences only, no GPL/LGPL/AGPL); the packages are exactly the
  closure of the requirements; and the notices were written from this lock. The xtask test
  `real_lock_passes` runs the same check, so `cargo xtask check` runs it too.
- `cargo xtask python-runtime check --online --advisories advisories.json`: the nightly audit
  (`--advisories`, which writes what OSV found as JSON, is optional). Every pinned wheel must
  still be on PyPI with the same sha256 and size, not yanked; every PBS archive must still be in
  its release's `SHA256SUMS` with the same sha256; and OSV is asked for advisories against each
  pinned version (reported, not failing). pandoc's release must still be immutable and give each
  archive the same sha256 and size, and its source tarball must hash the same.
- `cargo xtask python-runtime bump --resolver-python python3 --cache /tmp/pyrt-cache`: re-resolves
  `[resolve] requirements` with pip for each target (binary wheels only), takes every sha256 from
  PBS's `SHA256SUMS`, pip's report and PyPI, takes pandoc's archives for `[pandoc] version` (each
  sha256 from GitHub's API) and its source tarball, and rewrites `lock.toml` and
  `THIRD-PARTY-NOTICES.txt`. It needs the network, a Python with pip 23.1 or newer (nothing is
  installed into it), and a `tar` that reads zstd (for PBS's licence files). Both flags are
  optional: the resolver defaults to `python3` (`python` on Windows), the cache to a temporary
  folder.
- `cargo xtask python-runtime build --target aarch64-apple-darwin --out /tmp/pyrt-out`: builds
  one target's archive, on a machine of that target. It checks every input before reading it (PBS's `SHA256SUMS`, each sha256 and size, PBS's Sigstore build
  attestation with `gh attestation verify`, and pandoc's release attestation with `gh release
  verify-asset`; gh 2.75 or newer and a token), unpacks PBS, installs the wheels offline with
  `--require-hashes`, runs `pip check`, trims what the runtime does not need, copies the pandoc
  binary in, checks that every licence file inside (and pandoc's) is in the notices word for
  word, compiles the bytecode (`unchecked-hash`, so nothing writes `__pycache__` later), runs
  the smoke test (which also runs `pandoc --version` and a .docx to Markdown and back), packs
  the archive and writes `build.json` in the `--out` folder (name, sha256, size, unpacked size).
  `--download-only` stops after the checks, on any machine. `--no-attestation` is for a local try
  without gh; CI never passes it.

## What the runtime drops

Tcl/Tk with tkinter, IDLE, turtle, ensurepip, CPython's test modules, headers and import
libraries, macOS's `libpython3.13.dylib` (PBS's `python3.13` is linked statically and never loads
it), and the test suites inside numpy and pandas (about 53 MB unpacked). Their notices are left out
of `THIRD-PARTY-NOTICES.txt` accordingly. pip stays.

## Bumping

1. Edit `[runtime]` (a new CPython or PBS release), `[resolve]` (a requirement or its bound) or
   `[pandoc] version` (a new pandoc release: `gh release view -R jgm/pandoc`) by hand, or change
   nothing to pick up new versions within the bounds.
2. Run `cargo xtask python-runtime bump --cache /tmp/pyrt-cache`. When a package's metadata has no
   `License-Expression`, bump stops and asks for its SPDX licence in `[resolve.licences]`: read the
   package's licence files and add it.
3. Review the diff of `lock.toml` and `THIRD-PARTY-NOTICES.txt`. The build number goes up by one
   whenever an input changed.
4. Open a pull request. The workflow checks the lock and builds and smoke-tests all four archives.
5. After the merge, run the workflow on main with `publish`. It refuses if the release or tag for
   this build number exists.

Never edit a sha256, size, file name or URL by hand: `check` fails a lock that `bump` did not lay
out, and `build` fails any download whose sha256 differs.
