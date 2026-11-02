# 0035: Release builds from one tag

## Context

Clauseo ships as a DMG for each Mac architecture, two NSIS installers for each Windows architecture
(`currentUser`, the default download that updates itself, and `perMachine`, for firms whose IT
updates it), and a `.deb` and an `.rpm`. The updater (`tauri-plugin-updater`) reads a static
`latest.json` on GitHub Releases and checks each download's minisign signature, whose trusted comment
must name the version (`requireSignedVersion`). Signing is a later concern: macOS is signed ad hoc
until the Apple account exists, Windows is unsigned until SignPath approves. SignPath has to sign the
Windows exe between the link and the bundling, so `tauri-action`, which does both in one step, does
not fit. The release secrets live only in GitHub environments, and no workflow that holds one may run
for a pull request.

## Decision

- **One workflow,** `.github/workflows/release.yml`, started only by a pushed `v*` or `test-v*` tag
  and by `workflow_dispatch`. Its `check` job sets the mode:
  - `release`: tag `vX.Y.Z` on a commit on `main`. The jobs that sign use the environment `release`;
    `assemble` leaves a draft GitHub Release; `publish` (environment `release-publish`, Rohan as
    required reviewer) refuses unless every DMG passes `xcrun stapler validate`, then publishes it.
  - `test`: tag `test-vX.Y.Z` on a commit of a `release-test/*` branch (a branch from `main` with only
    the version bumped, deleted after the run). Built with the `e2e` feature under the environment
    `release-test`, its updater files signed with the throwaway test key, published as a pre-release,
    since a draft's assets need a signed-in download.
  - `dry`: `workflow_dispatch` (input `dry_run`, default and only accepted value `true`), from any
    ref. Decided with Rohan's coordinator (2026-11-02): a dry run uses no environment and no secret.
    Each job that signs makes a throwaway updater key with a random password in its own runner and
    signs with it, and nothing is published: the assets, `latest.json` and `SHA256SUMS.txt` stay in
    the run's artifact `release-assets`. Since a dry run holds nothing secret, it may start from any
    branch, so a fix can be dry-run before it merges (`gh workflow run release.yml --ref <branch>`).
- **Jobs:** `check` (mode, tag against version and branch, the release overlays, no test feature in a
  `v*` build); `ui` (the ui built once for every leg, with hidden source maps moved out of
  its `dist` folder before any leg embeds it); `build` (the matrix below); `sign-windows` (SignPath's slot, then both NSIS
  installers from the one exe with `tauri bundle --no-binary-patching`, then the updater signature,
  last); `assemble` (names the Mac archives `Clauseo_<v>_<arch>.app.tar.gz`, writes `latest.json` and
  `SHA256SUMS.txt`, attests every asset, creates the Release); `publish`; `measure-builds` (the
  `marks` build on macOS and the `e2e` build on Windows for the laptop pass, never on a Release).
- **Workflow rules:** every action pinned by commit SHA; `permissions:` on every job (none at the
  top); each secret on the one step that uses it; `pnpm install --frozen-lockfile` and `cargo fetch
  --locked` before any secret; no cache restored or saved. The updater key is present only in the
  step that bundles the Mac `.app` and the step that signs the Windows installer, never while
  third-party build scripts run. Only the macOS legs of `build` take an environment, since only they
  sign there. `.github/scripts/latest-json.test.mjs` fails on a pull request trigger, an unpinned
  action, a job without `permissions:` or a secret outside a step, and runs in CI's fast lane.
- **Artefacts by tier.** Tier 1, which must build: `Clauseo_<v>_aarch64.dmg` and
  `Clauseo_<v>_aarch64.app.tar.gz` (`macos-15`), `Clauseo_<v>_x64-setup.exe` and
  `Clauseo_<v>_x64-machine-setup.exe` (`windows-2025`). Tier 2, `continue-on-error`, named in the
  release notes when missing: the same for Intel Mac (`x64`, cross-built on `macos-15`) and Windows
  ARM64 (`arm64`, `windows-11-arm`), and `Clauseo_<v>_amd64.deb` and `Clauseo-<v>-1.x86_64.rpm`
  (`ubuntu-22.04`, the oldest glibc we support). The `.app.tar.gz` and the `currentUser` installers
  carry a `.sig`; Linux never updates itself, so its packages have none and its leg holds no secret.
  `latest-json.mjs` is the one home of this table.
- **The base config** (`crates/desktop/tauri.conf.json`) holds what every build shares: the bundle
  targets, `createUpdaterArtifacts: true`, macOS 13 and the hardened runtime, the WebView2 bootstrapper
  (silent) with `minimumWebview2Version` `155.0.0.0` (Microsoft Edge 155.0.4283.62 on the
  `windows-2025` image 20261021.299.1, actions/runner-images `Windows2025-Readme.md`; raised only on
  purpose), NSIS `currentUser` in English and Hindi, `allowDowngrades: false`.
- **Release overlays,** in `crates/desktop/release/`, passed with `--config`:
  `release.conf.json` (`mainBinaryName: "Clauseo"`, and no `beforeBuildCommand`, since the legs embed
  the `ui` job's build); `windows.conf.json` (`bundle.windows.nsis.installerHooks`, the uninstall hook
  in `crates/desktop/windows/hooks.nsh`, and later S15's sandbox tools in `bundle.externalBin`);
  `machine.conf.json` (`installMode: "perMachine"`). `check` fails on any other key in them. The guard
  covers the overlays only: sidecars every platform ships (S27's `clauseo-mcp` and `agent-browser`)
  live in `tauri.conf.json`, where the `tauri-config` check and review cover them.
- **The sidecar seam.** Each `build` leg has one step, "Sidecars for this target", before the app is
  built (tauri-build checks that every `externalBin` file exists): it builds or fetches that leg's
  sidecars for its target triple as `crates/desktop/binaries/<name>-<target>`. The Windows legs carry
  them to `sign-windows` in the `windows-exe-<target>` artifact, and the size summary lists each
  beside the app. A sidecar task adds its build or fetch there and nothing else moves. A
  `beforeBundleCommand` would run inside the Mac bundling step, which holds the updater key, so
  sidecar work belongs in the seam instead.
- **The version** is `X.Y.Z` only, in `crates/desktop/Cargo.toml` and `tauri.conf.json`;
  `crates/desktop/tests/version.rs` fails when they differ, and `check` fails when a tag names another
  version. The first release is `0.1.0`.
- **The release date** (`YYYY-MM-DD`, UTC: the annotated tag's date, else the tagged commit's) is
  compiled in as `CLAUSEO_RELEASE_DATE` for `release` and `test` builds only; `build.rs` fails on a
  malformed value, and `display_info` returns it as `release_date`, `null` in dev, CI and dry-run
  builds.
- **`latest.json`** is `{ version, notes, pub_date, platforms }`, with `darwin-aarch64-app`,
  `darwin-x86_64-app`, `windows-x86_64-nsis` and `windows-aarch64-nsis`; each Windows entry is
  repeated under plain `windows-<arch>`, since an exe bundled with `--no-binary-patching` carries no
  bundle type and the plugin then looks up `<os>-<arch>`.
- **The updater's public keys.** The app's key is compiled in from `plugins.updater.pubkey` and the
  release jobs never change it. The bundler's signing step is told the public key of the key that
  signs: the throwaway key's in a dry run, the test key's (a file `updater-test.pub` in the release
  folder, which the updater task commits) in a test, the app's own in a release. `check` stops a
  `release` or `test` run that lacks it.
- **Release flags**, in `release.yml` only: `-C force-frame-pointers=yes` on every target, plus
  `-C link-arg=-Wl,--build-id=sha1` on Linux; `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` and
  `CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=packed`. The shipped binaries stay stripped. The `.dSYM`,
  `.pdb` and `.dwp` go up as `debug-files-<target>`, the ui's source maps as
  `ui-sourcemaps-<version>`, for the crash reports' symbols job; neither is ever bundled or released.
- **Who may tag and use the secrets:** Rohan, as repository admin, applies a tag ruleset (only he
  creates `v*` and `test-v*` tags) and the environments' branch and tag rules.

## Consequences

- One commit gives every asset, and `sha256sum -c SHA256SUMS.txt` checks a download. Every asset has
  a build provenance attestation.
- A release builds cold on every leg (no cache), about the slowest a build can be; a release is rare.
- Dry runs cost no secret and can be started by anyone with write access, from any branch, and their
  signatures prove the pipeline, not trust: each leg signs with its own throwaway key.
- Both Windows installers ship the same exe, so SignPath signs once, and the app tells a per-machine
  install at run time. The NSIS hook removes S15's set-up only on a real uninstall, never during an
  update.
- S20a fills the two SignPath steps (skipped until `vars.SIGNPATH_ENABLED` is `true` on a `v*` tag,
  and failing if turned on before they exist), the Apple secrets and notarisation. The crash
  reports' task adds the `symbols` job, the usage-counts task the release-recording script (its step
  is skipped while `vars.COUNTS_RELEASES_ENABLED` is not `true`), and the updater task the keys and
  `requireSignedVersion`.
- The binary is called `Clauseo` only in release builds; dev and CI builds keep `clauseo-desktop`, so
  the other workflows' paths stay as they are.

## Status

proposed

## Date

2026-11-02
