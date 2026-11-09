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

**Signing for the beta.** Rohan, 2026-11-05: Windows ships unsigned for the beta; SignPath (S20a)
stays deferred until there are users and funding. macOS will be notarised under an individual
Developer ID.

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
    the run's artifact `dist-assets`. Since a dry run holds nothing secret, it may start from any
    branch, so a fix can be dry-run before it merges (`gh workflow run release.yml --ref <branch>`).
- **Jobs:** `check` (mode, tag against version and branch, the release overlays, no test feature in a
  `v*` build); `ui` (the ui built once for every leg, with hidden source maps moved out of
  its `dist` folder before any leg embeds it); `build` (the matrix below); `sign-windows` (Authenticode when it is on, see S20a below, then both NSIS
  installers from the one exe with `tauri bundle --no-binary-patching`, then the updater signature,
  last); `assemble` (names the Mac archives `Clauseo_<v>_<arch>.app.tar.gz`, writes `latest.json` and
  `SHA256SUMS.txt`, attests every asset, creates the Release); `publish`; `measure-builds` (the
  `marks` build on macOS and the `e2e` build on Windows for the laptop pass, never on a Release).
- **Workflow rules:** every action pinned by commit SHA; `permissions:` on every job (none at the
  top); each secret on the one step that uses it; `pnpm install --frozen-lockfile` and `cargo fetch
  --locked` before any secret; no cache restored or saved. The updater key is present only in the
  step that bundles the Mac `.app` and the steps that sign the Windows sandbox tool and installer, never while
  third-party build scripts run. Only the macOS legs of `build` take an environment, since only they
  sign there. `.github/scripts/latest-json.test.mjs` fails on a pull request trigger, an unpinned
  action, a job without `permissions:` or a secret outside a step, and runs in CI's fast lane.
- **No test key in a shipped binary.** In every `build` leg of a run that is not `test` (so
  release and dry runs), the step right after `tauri build`, before any signing or upload (on
  Linux, `tauri build --bundles deb,rpm` has already made the packages), scans the app binary as
  linked (`Clauseo.exe`, or `Clauseo`, which becomes the `.app`'s main executable) for the
  throwaway test key (`updater-test.pub`): its base64 text, its `RW…` key line and its key id, in ASCII or UTF-16 (NULs dropped first). A hit fails the
  leg with `::error::` naming the file. A `test` run builds `e2e`, which holds the key on
  purpose. The desktop tests' own checks (0037) cover only the source text and the test binary.
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
  purpose), NSIS `currentUser` in English, `allowDowngrades: false`.
- **Release overlays,** in `crates/desktop/release/`, passed with `--config`:
  `release.conf.json` (`mainBinaryName: "Clauseo"`, and no `beforeBuildCommand`, since the legs embed
  the `ui` job's build); `windows.conf.json` (`bundle.windows.nsis.installerHooks`, the uninstall hook
  in `crates/desktop/windows/hooks.nsh`); `machine.conf.json` (`installMode: "perMachine"`);
  `blocklist.conf.json` (`bundle.resources`, the block-list snapshot: see "The block-list snapshot"
  below). `check` fails on any other key in them. Sidecars are in none of them: see "Sidecars" below.
- **The sidecar seam.** Each `build` leg has one step, "Sidecars for this target", after the installs
  and before the app is built (tauri-build checks that every `externalBin` file exists). It runs
  `.github/scripts/sidecars.mjs` for the leg's target ("Sidecars" below). A sidecar task adds a row to
  that script's table and nothing else moves. A `beforeBundleCommand` would run inside the Mac
  bundling step, which holds the updater key, so sidecar work belongs in the seam instead.
- **The version** is `X.Y.Z` only, in `crates/desktop/Cargo.toml` and `tauri.conf.json`;
  `crates/desktop/tests/version.rs` fails when they differ, and `check` fails when a tag names another
  version. The first release is `0.1.0`.
- **The release date** (`YYYY-MM-DD`, UTC: the annotated tag's date, else the tagged commit's) is
  compiled in as `CLAUSEO_RELEASE_DATE` for `release` and `test` builds only; `build.rs` fails on a
  malformed value, and `display_info` returns it as `release_date`, `null` in dev, CI and dry-run
  builds.
- **The test build** is `display_info`'s `test_build`, true exactly in a build with the `e2e`
  feature (`test-v*` pre-releases ship with it, publicly). The rail's foot and Settings' About then
  show a muted "Test build" label; the window title does not change.
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

### PR 2: the WebView2 dialog and the installer's words (Rohan, 2026-11-03)

- **The WebView2 dialog** (`crates/desktop/src/webview2.rs`). On Windows, `run` first reads the
  installed runtime's version (`tauri::webview_version`, wry's call to
  `GetAvailableCoreWebView2BrowserVersionString`; no new crate). When it is missing or older than
  `minimumWebview2Version`, a task dialog shows the copy page's W1 (title), W2 (main instruction),
  W4 (content) and W5 (footer), with W6 "Download WebView2" (opens
  `https://go.microsoft.com/fwlink/p/?LinkId=2124703` in the default browser, then exits) and W7
  "Close" (exits); `run` returns without building the app, so there is never a blank window. W3,
  the line with a time estimate, waits for QU.8. The minimum lives in `tauri.conf.json`; the
  module's copy is held to it by a test. A version string it cannot read lets the app start, so
  the check never refuses a runtime that may work.
- **English and Hindi in the installer** (stage F, 2026-11-07; the dialog is still English
  only). The installer's `languages` is `["English", "Hindi"]`, English first as NSIS's
  fallback, with no language picker. Tauri 2.12.2 has no Hindi of its own, so
  `crates/desktop/windows/nsis/Hindi.nsh` is ours: the same keys as `English.nsh` in the same
  order, which tests hold equal, and a test that fails until every value holds Devanagari (the
  app thread's provisional Hindi replaces the English placeholders). On a Tauri upgrade, copy its
  new template keys into both files. The language is chosen by
  `HKCU\Software\Clauseo\UiLanguage` (`en` or `hi`, which the app writes), then by Windows'
  display language: NSIS fixes the pages' language right after `.onInit` and `un.onInit`, and
  Tauri's template gives neither a hook. So the installer follows `UiLanguage` when it is set,
  else Windows' (2026-11-08): the template's block `ui-language` in `.onInit` inserts
  `hooks.nsh`'s `CLAUSEO_UI_LANGUAGE`, before the pages' language is fixed. A first install has
  no value yet, so Windows decides; a repair, a re-run and the updater's `/UPDATE` re-run follow
  the app. A per-machine install elevated as another account reads that account's value. The
  uninstaller follows the value `hooks.nsh`'s `NSIS_HOOK_POSTINSTALL`
  writes at every install and update (the template's `Installer Language`, which its
  `un.onInit` reads): `UiLanguage` then, else the installer's own language. Without that value
  a two-language uninstaller would show NSIS's picker. Deleting the data on uninstall deletes
  `UiLanguage` too; keeping it keeps the value.
- **Stock installer pages.** Tauri's MUI pages as they come, with no page words of our own. The
  installer is built from `crates/desktop/windows/nsis/installer.nsi`
  (`bundle.windows.nsis.template`): a byte-identical copy of tauri-cli 2.12.2's template plus
  nine marked blocks (S15 tier B, 2026-11-07; `ui-language`, above, 2026-11-08).
  `admin-section`, at the top of `.onInit`, runs the per-user installer's elevated instance
  (`/CLAUSEO_ADMIN_SECTION`) and quits before any plug-in, page or section; in the per-machine installer it first makes the run's own temporary folder
  under `%ProgramFiles%\Clauseo\installer-temp`, locked to Administrators and SYSTEM, and points
  `$PLUGINSDIR` and `$TEMP` at it (`safe_temp.nsh`, decision 0049). The hooks alone couldn't do
  either: the template's `.onInit` calls plug-ins (`${RunningX64}` in `SetContext` on every run,
  `SemverCompare` under `/S`), which NSIS extracts into `$PLUGINSDIR` under `%TEMP%`, a folder
  the lawyer's processes can rename while the elevated instance is about to load from it.
  `no-solid` drops `/SOLID`, so the stub reads each file straight from the installer instead of
  unpacking all its data into a file in `%TEMP%` first, which the elevated instance would read
  the sandbox's set-up program from. `finish-show` and `finish-show-function` hide "Run Clauseo"
  after a set-up wait that timed out. The per-machine uninstaller's four (`uninstall-string`,
  `un-admin-section`, `uninstall-self-delete`, `un-gui-end`) register it with `_?=` so it runs in
  place, give it the same temporary folder, and delete it once it has exited (decision 0049,
  `uninstall_in_place.nsh`). A test holds the copy to upstream plus a known list of
  named blocks (by SHA-256 of upstream's text, rebuilt with the blocks taken out and the lines
  they stand in for put back) and to the CLI version in
  `ui/package.json`, so CI fails when either moves. Every Tauri upgrade copies the new template
  again, puts the blocks back and updates the pin. The words list's per-machine lines are parked
  with it. The uninstall-declined line is S15a's.
- **One installer line of our own.** Tauri's "Delete the application data" box removes the whole
  data folder, store included, so its label says what goes: "Also delete conversations, saved
  versions and settings" (unticked, as Tauri draws it). `customLanguageFiles` replaces the
  bundler's whole English file, so `crates/desktop/windows/nsis/English.nsh` holds that line
  first and Tauri's 26 other messages as 2.12.2 ships them, except that three name the product
  with `${PRODUCTNAME}` where Tauri's file has a literal `{{product_name}}` (language files are
  included, not rendered). On a Tauri upgrade, copy its `English.nsh` again and keep that fix;
  tests count the keys (37: Tauri's 27 and Clauseo's 10, in both language files), refuse any
  `{{`, and hold the label to one line (60 characters; the box is 400 x 25 px).
- **The dialog's look** (from the Windows 11 VM's run): `cxWidth` 280 dialog units, so the main
  instruction fits on one line, and the exe's own icon (tauri-build's
  `WINDOWS_APP_ICON_RESOURCE_ID`) as its main and title-bar icon, named only when the exe holds
  that resource. When `TaskDialogIndirect` fails, a plain message box says the title, the main
  line and the body, with OK, and the app exits as after Close.

## Consequences

- One commit gives every asset, and `sha256sum -c SHA256SUMS.txt` checks a download. Every asset has
  a build provenance attestation.
- S15's sandbox tool (release.yml's `SANDBOX_EXE`, `clauseo-sandbox-setup.exe`) is signed in `sign-windows` with the mode's updater key, after
  Authenticode and before NSIS, the same key and per-mode handling as the currentUser installer. A
  generated `bundle.resources` overlay puts its `.sig` beside it in the install folder of both
  installers (NSIS installs resources and `externalBin` alike under `$INSTDIR`), and both go on the
  Release as `<name>-<arch>.exe` and `.exe.sig`, in `SHA256SUMS.txt` like every asset. `assemble`
  checks the `.sig` (and the currentUser installer's) against the mode's updater public key (a dry
  run's comes from each Windows leg as the artifact `dry-updater-pub-<target>`), opens both
  installers with 7-Zip (`p7zip-full`) and fails unless each holds exactly the published exe and
  `.sig` at the install folder's root. It also reads the tool's VERSIONINFO string `UpdaterKeyId` (stage C)
  by parsing its PE resources and fails unless it is the mode's key ID: test's `updater-test.pub`,
  release's `tauri.conf.json` pubkey, dry's the same when set, else `build.rs`'s placeholder
  `AC986E7C6BA67AB9`. Until stage C's PR 3 merges an exe without it only warns. The release notes list its SHA-256 per architecture under
  "Inside the Windows installers", for IT to check a copy restored from quarantine (firm-it §10).
- The `build` job sets `CLAUSEO_RELEASE_MODE` and, for a release or a test run, `CLAUSEO_UPDATER_PUBKEY`
  (public keys only) for the sandbox's `build.rs`; a dry run sets no key, so its refresh fails closed.
- A release builds cold on every leg (no cache), about the slowest a build can be; a release is rare.
- Dry runs cost no secret and can be started by anyone with write access, from any branch, and their
  signatures prove the pipeline, not trust: each leg signs with its own throwaway key.
- Both Windows installers ship the same exe, so SignPath signs once, and the app tells a per-machine
  install at run time. The NSIS hook removes S15's set-up only on a real uninstall, never during an
  update.
- S20a's Windows signing is built and off ("Windows code signing" below); what is left is SignPath's
  approval, its settings and the switch, then the Apple secrets and notarisation. The crash
  reports' task adds the `symbols` job, the usage-counts task the release-recording script (its step
  is skipped while `vars.COUNTS_RELEASES_ENABLED` is not `true`), and the updater task the keys and
  `requireSignedVersion`.
- The binary is called `Clauseo` only in release builds; dev and CI builds keep `clauseo-desktop`, so
  the other workflows' paths stay as they are.

## Linux and sizes (S20 follow-up, 2026-11-03)

- Rohan chose performance over size: `[profile.release]` stays at opt-level 3 for every package. The
  Linux build drops `tauri-plugin-updater`, which Linux never uses (R23: Linux updates through its
  package manager). The plugin is a dependency outside Linux (`crates/desktop/Cargo.toml`) and is
  registered only there (`crates/desktop/src/lib.rs`); `tauri.conf.json`'s `plugins.updater` stays.
- The Linux budgets are the first Linux dry run's sizes plus 5% (`budgets.toml`, release run
  41660337744): `release_binary_size` 23.84 MB measured, limit 25.1 MB; `deb_size` 10.90 MB measured,
  limit 11.5 MB. Windows and macOS have no size rows.

## Sidecars (S20 follow-up, 2026-11-04)

The plan is the private repo's `tasks/stage-f/S20-sidecars.md`. This changes PR 1's plan, in which
S15's tools sat in `windows.conf.json` and S27's in `tauri.conf.json`.

- **No `externalBin` in the base config.** tauri-build checks at `cargo build` time that every
  listed file exists, so a sidecar in `tauri.conf.json` would break every dev, CI, e2e and
  scoreboard build. Release builds get the list from a generated overlay instead.
- **One script, `.github/scripts/sidecars.mjs <target-triple>`** (Node built-ins only), holds the
  table of sidecars and their targets. For one target it builds or fetches each, copies it to
  `crates/desktop/binaries/<name>-<triple>` (`.exe` on Windows; Tauri's naming), and writes
  `sidecars.generated.json` in `crates/desktop/release/`, holding only `bundle.externalBin` for the
  sidecars it placed. Both are gitignored. Every name starts with `clauseo-` (the `.deb` and
  `.rpm` put sidecars in `/usr/bin`, where a bare `agent-browser` from a global npm install
  could already be); the script refuses a row without it. Today's rows, every release target:
  - `clauseo-mcp`: `cargo build --release --locked -p clauseo-mcp --target <triple>` with the job's
    own `RUSTFLAGS` and profile variables, so it gets the app's release flags; its `.pdb`, `.dSYM`
    or `.dwp` goes into `debug-files-<target>` with the app's.
  - `clauseo-agent-browser`: `fetchAgentBrowser` from `crates/desktop/sidecars/agent-browser.mjs`,
    checked against `crates/desktop/agent-browser.toml` (upstream's asset names; only the
    installed name has the prefix). A triple the pin marks `none = true` leaves it out of that target's list; a
    triple missing from the pin fails the job. agent-browser's image-name check
    (`connection.rs:198`, `is_agent_browser_process`, which wants a name starting with
    `agent-browser`) then skips its stale-daemon kill: on Windows the guard's Job Object (#329)
    covers that; on Unix it is the known G17 item 6 limit (an orphan lasts until its idle timeout).
  - `clauseo-guard` is a commented-out row: parked (host thread, 2026-11-03). S15 adds
    `clauseo-sandbox-setup` and `clauseo-sandbox-run` (Windows targets) as rows of their own.
- **The overlay goes last.** Every `tauri build` and `tauri bundle` of a shipped leg passes
  `--config release/sidecars.generated.json` after every other `--config`. A JSON merge patch
  replaces arrays, so one generated list per target is the only safe shape. `sidecars.mjs --check`,
  after the seam and again in `sign-windows` after the download, fails if the file holds anything
  but `bundle.externalBin` or an entry not under `binaries/clauseo-`; `check` now allows only
  `bundle.windows.nsis.installerHooks` in `windows.conf.json`. `latest-json.test.mjs` fails when a
  step running the script, or any step before it in the job, holds a secret, and when a shipped
  build passes the overlay other than last. The measuring builds (S24) ship nothing and take no
  sidecar.
- **Windows.** The `build` leg uploads the sidecars and the overlay with `Clauseo.exe` in
  `windows-exe-<target>`, and `sign-windows` downloads them back to the same two folders in
  `crates/desktop/` before `tauri bundle --no-binary-patching`, so both installers take
  exactly the files that were built. Once signing is on (S20a) every PE there and `Clauseo.exe` is
  Authenticode-signed before NSIS and before the updater signature.
- **Where they land:** beside the app's exe in every bundle: `Clauseo.app/Contents/MacOS/`,
  `/usr/bin/` in the `.deb` and `.rpm` (hence the `clauseo-` prefix), and the install folder of
  both NSIS installers, which install and remove each `externalBin` with `Clauseo.exe`.
- **Sizes.** The size table lists each sidecar on its own row beside the app's exe; none counts
  towards `release_binary_size`. On Linux each has its own budget row (`agent_browser_size`,
  `clauseo_mcp_size`), `deb_total_size` is the .deb as shipped, and `deb_size` stays the app's own
  payload: the .deb repacked without the sidecars (orchestrator, 2026-11-04).
- **Per-machine sandbox grants, with S15 (not before).** Under `%ProgramFiles%` S15's launcher cannot
  write ACEs without admin, so the elevated per-machine installer writes them in `hooks.nsh`'s
  `NSIS_HOOK_POSTINSTALL`, per-machine only, on every install and upgrade (a recreated file loses
  its ACE; per-machine installs never update themselves). Per-user installs skip it. Each grant is
  per file, without inheritance, read and execute only, to the well-known `RESTRICTED` SID:
  `icacls "$INSTDIR\<file>.exe" /grant:r *S-1-5-12:(RX)` (`Everyone` if S15's spike drops
  `RESTRICTED` from the restricting list). The files are what a jailed process starts:
  `Clauseo.exe` (and any non-system DLL beside it that it imports), `clauseo-agent-browser.exe`,
  `clauseo-mcp.exe` (if the agent starts it from its MCP config), and `clauseo-guard.exe` only if
  the guard returns. Never `clauseo-sandbox-setup.exe` (a jailed agent could raise a real UAC
  prompt), and not `clauseo-sandbox-run.exe` (it runs with the plain logon token). Uninstall has
  nothing to do: the ACEs go with the folder.

## Uninstall deletes app data without following links (S20, 2026-11-05)

- **The bug.** Stage C's Windows VM run (S15 PR3, row 29) put a directory junction under the data
  folder pointing at a folder outside it, then ticked the uninstaller's "Also delete
  conversations, saved versions and settings". Tauri's template deletes the data folders with
  `RmDir /r` (tauri-cli 2.12.2, `installer.nsi` lines 877-891), and NSIS 3.11's `RMDir /r`
  follows junctions: the outside folder was emptied. `rmdir /s /q` and `Remove-Item -Recurse`
  did not follow it. Agent tools, npm or pnpm links and tier B's set-aside can all leave a
  junction there.
- **The fix.** `hooks.nsh`'s `NSIS_HOOK_PREUNINSTALL` runs before the template's own steps
  (lines 783-788). It first runs the template's `CheckIfAppIsRunning`, so a declined prompt
  stops the uninstall before anything is removed. Then S15's `--remove`, now through
  `ShellExecuteExW` so its exit code is read. Then, for the ticked box, it clears
  `$DeleteAppDataCheckboxState`, so the template's `RmDir /r` never runs, and runs
  `Clauseo.exe --delete-app-data`, adding `--sandbox-removed` only when `--remove` exited 0.
- **What the app deletes.** It takes no path: it deletes `FOLDERID_LocalAppData` and
  `FOLDERID_RoamingAppData` joined with `ai.clauseo.desktop`. It refuses a data folder that is a
  reparse point or resolves elsewhere. It lists a folder only while holding it open by a handle
  that refuses a reparse point and shares no delete or write access. Each entry goes by std alone:
  a link as a link, a folder with `remove_dir_all`, a file with `remove_file`. It walks by hand
  only toward what it keeps.
- **What it keeps.** `sandbox` (S15's records), unless `--sandbox-removed`, and
  `sandbox\set-aside` whenever it holds anything but Clauseo's own bookkeeping: a matter's
  folder holding only `record.json` with no open session, `set-aside.lock`, and a
  `manifest.json` listing no item still set aside, each a plain file with one link, counts as
  empty and goes, by handle (`crates/desktop/src/delete_app_data/set_aside.rs`).
- **Exit codes and messages.** 0 deleted; 2 or 3 a folder kept and all else gone; 4 a folder
  kept and something else left; 1 refused or something left with nothing kept. The uninstaller
  shows one box unless silent (`English.nsh`): `deleteAppDataKept` naming the kept folder,
  `deleteAppDataKeptFailed` naming it and the data folder, or `deleteAppDataFailed` naming the
  data folder when nothing was kept. It never falls back to `RMDir /r`. `crates/desktop/src/delete_app_data.rs` holds the contract.
- **Follow-ups from the VM rows (2026-11-05).** A `sandbox` that is itself a junction or a
  directory symbolic link, with no `--sandbox-removed`, is no longer kept: the app opens it by
  handle without following it, reads its reparse tag on that handle, and deletes the link through
  the same handle only when it is a junction or a symbolic link, so a real folder swapped in is
  never deleted. Any other reparse point (a cloud placeholder) is kept. Either way the run fails
  (exit 1) and the box names nothing outside the data folder. The hook's `ShellExecuteExW` adds
  `SEE_MASK_FLAG_NO_UI`, so a declined or blocked elevation (1223, 1260) shows no error box of
  Windows' own; the consent prompt still shows, and a denial keeps `sandbox\` as a failed
  `--remove` does. The hook names a kept folder only when `GetFileAttributesW` says a real
  folder, and shows no box when the data folder is gone (exit 1 after an unlink-only run); when
  the data folder is itself a link (refused, so kept) its box names no path.

## The block-list snapshot (stage F, 2026-11-07)

- **Why it is bundled.** The app (`crates/blocklist`) enforces a signed list of unsafe sites, published
  as clauseo-ai/blocklist's `current` release (`blocklist-v1.txt.gz` and its `.minisig`). Every
  release, test and dry build ships the snapshot current at build time, at
  `<resource_dir>/blocklist/` on all three platforms, so a fresh install has a list from its first
  start. Dev, CI, e2e and smoke builds carry none; the app handles that.
- **Verify before ship.** The `blocklist` job, in every mode, downloads both files and runs
  `minisign -V` with `crates/blocklist/keys/stage-f.pub`, the key the app compiles in. The run fails
  if the signature does not verify or if the signed trusted comment is not exactly
  `clauseo-blocklist v1 generated=<YYYY-MM-DDTHH:MM:SSZ> entries=<n>`. Only then does it upload
  `blocklist-snapshot`, which `build`, `sign-windows` and `measure-builds` download into a
  gitignored `blocklist` folder in the desktop crate before they build or bundle. Each of their `tauri build`
  and `tauri bundle` runs that passes `release.conf.json` passes `blocklist.conf.json` too, and
  `latest-json.test.mjs` fails when one does not. The `--no-bundle` compiles need the files as well:
  tauri-build copies every `bundle.resources` entry at compile time and fails on a missing one.
  In `sign-windows` the sandbox tool's generated `bundle.resources` overlay comes after it; a JSON
  merge patch merges objects, so the `.sig` entry joins the two snapshot entries.
- **Stale is a warning.** The job writes `generated`, `entries` and both files' SHA-256 to its
  summary. A snapshot generated more than 24 hours before the run adds a warning there, and the run
  goes on: an older signed list is still better than none.

## Windows code signing, ready behind a switch (S20a, 2026-11-08)

Windows ships unsigned for the beta (Context). The pipeline to sign it is in place and off, so turning
it on is settings, not code.

- **The switch.** `check` sets the output `signing`: `signpath` for a `v*` or `test-v*` run once the
  repository variable `SIGNPATH_ENABLED` is `true`; `selfsigned` for a dry run started with the input
  `sign_test_selfsigned` (default false); else `none`. A dry run never uses SignPath. With `none`,
  every new step is skipped and the bundle step sets none of the template's variables, so the
  installers are built exactly as before. `SIGNPATH_ENABLED` must be a repository variable, not an
  environment one: `check` and `assemble` have no environment.
- **What is signed, in `sign-windows`, in this order.** (1) `Clauseo.exe`, every sidecar the build leg placed
  (clauseo-mcp, clauseo-agent-browser, the sandbox tool) and every NSIS plug-in. The plug-ins come from the bundler's NSIS toolset (`%LOCALAPPDATA%\tauri\NSIS`, fetched by a
  throwaway uncompressed bundle on the cold runner). The step signs copies of every DLL in
  `Plugins\x86-unicode` and its `additional` folder: System, nsDialogs, StartMenu, LangDLL, NSISdl,
  nsis_tauri_utils and the rest. It never signs the toolset's own files: the bundler re-downloads a
  `nsis_tauri_utils.dll` whose hash changed. The template's `signed-plugins` block adds the copies'
  folder last when `CLAUSEO_SIGNED_PLUGINS=1` (makensis takes a plug-in from the last folder that has
  it). (2) The sandbox tool's updater `.sig`. (3) The uninstaller, in two passes: a first bundle of
  each installer runs the template's `sign-uninstaller` block (`!uninstfinalize`, compiled in only when
  `CLAUSEO_SIGN_UNINSTALLER=1`), which captures the uninstaller with
  `.github/scripts/nsis-uninstaller.mjs`. Both captures are signed. The installers' own bundle, from
  the same inputs, then puts each signed uninstaller in place of the one it builds, and warns if the
  second build's uninstaller differs from the first. (4) Both installers. (5) The currentUser
  installer's updater signature, last. Each signing step ends with `latest-json.mjs --unsigned` over
  what it signed.
- **Each signed file is checked as it comes back, on Windows.** Before a folder goes to be signed,
  the action records each file's Authenticode digest (`latest-json.mjs --authenticode-digest`: the
  file's SHA-256 without its CheckSum, its certificate-table entry and the table, the parts signing
  changes). SignPath must return exactly the files sent, and each must have the digest it was sent
  with: a signed file can't be a different binary. Then `Get-AuthenticodeSignature` must give
  status `Valid` (the signature, the file's hash and the chain to a root Windows trusts all hold)
  and a signer certificate whose SHA-1 thumbprint is in `SIGNPATH_EXPECTED_SIGNER`.
- **The expected signer is pinned by thumbprint.** `SIGNPATH_EXPECTED_SIGNER` holds the SHA-1
  thumbprint (40 hex digits) of the certificate SignPath signs with, or several, space-separated,
  while a renewed certificate overlaps the old one. It names one exact certificate. A subject and
  issuer would also match any other certificate issued to the same name, and SignPath's
  open-source certificate is issued to SignPath Foundation, the same name for every project it
  signs. Renewing the certificate means adding its thumbprint before the old one expires, then
  removing the old one. A self-signed dry run checks against the throwaway certificate's thumbprint,
  which the job makes, and accepts only the untrusted-root status that certificate must give.
- **Where the full check runs.** The full check runs only on the Windows leg, where Windows' own
  verifier is. On Linux, `assemble` checks that every PE file in the assets and inside both
  installers has a signature. It also checks, in every mode, that each leg's `release-*` files
  are byte for byte what that leg uploaded (next point). So the files it publishes are the ones
  sign-windows checked.
- **Artifacts between jobs.** Each `windows-exe-*` and `release-*` artifact's files, every one
  (the machine installer, the sandbox tool and every `.sig` too), go from the uploading job to the
  next as a job output of SHA-256s (`.github/scripts/artifact-sums.mjs`), one output per matrix
  target. sign-windows checks its `windows-exe-*` download against build's, and assemble checks
  each `release-*` download against build's or sign-windows'. A missing, extra or changed file
  fails the job.
- **Why the two passes go through the action.** SignPath's open-source signing requires origin
  verification, which only its GitHub connector supplies: the official action
  (`signpath/github-action-submit-signing-request`, pinned) submitting a workflow artifact. A request
  from inside makensis through the REST API would carry no origin data. The local action
  `.github/actions/authenticode` uploads a flat folder, has SignPath sign it, waits (up to 40 minutes,
  for a policy that needs a person's approval) and puts the signed files back. The cost, per Windows
  architecture and only with signing on: three signing requests (exe, sidecars and plug-ins; the two
  uninstallers; the two installers), one uncompressed bundle and two full bundles more than today's two
  (five makensis runs instead of two). The job's limit rises from 30 to 180 minutes when signing is on:
  three waits of up to 40 minutes, 120 in all, and an hour for the rest of the job, twice its
  unsigned limit. A request that waits too long then fails with SignPath's own error, not by the job
  being cut off mid-wait.
- **The release check.** `assemble` unpacks both installers of each architecture with 7-Zip and runs
  `latest-json.mjs --unsigned` over the assets and the unpacked files. It finds PE files by their
  headers and treats a file as signed when its certificate table (data directory 4) holds a PKCS#7
  `WIN_CERTIFICATE`. It checks that a signature is there; whose it is, sign-windows has checked
  (above). It runs only with signing on: any entry fails the run before the Release is created, and
  so does an installer in which 7-Zip finds no `uninstall.exe`. With `none` it is skipped, so an
  error in 7-Zip or the script can't fail an unsigned release.
- **The plumbing proof.** `gh workflow run release.yml --ref <branch> -f sign_test_selfsigned=true`
  runs the same steps with a throwaway self-signed certificate (`New-SelfSignedCertificate` and
  `Set-AuthenticodeSignature` in Windows PowerShell, trusted by nothing) in place of SignPath. It proves
  the coverage, the capture and substitute, and the release check without SignPath. Dry runs without
  the input stay unsigned, as the Defender tests need.
- **Proving that off changes nothing.** Run a dry run with signing off on the branch and one on
  `main`, download each run's `dist-assets` (`gh run download <run-id> -n dist-assets -D <dir>`),
  then run `node .github/scripts/compare-installers.mjs <main-dir> <branch-dir>`. It unpacks each pair
  of Windows installers with 7-Zip, hashes every file inside both, and lists the differences in two
  groups. Expected from build to build: updater `.sig`s, the rebuilt uninstaller, and PE files that
  differ only in timestamps, debug IDs or the version resource. Everything else is unexpected,
  including the decompiled `[NSIS].nsi`, which it prints as a diff. The installers' bundle step logs
  `CLAUSEO_ variables set: none` after makensis's warnings check passes.
- **What is left.**
  1. SignPath's open-source application and approval.
  2. In SignPath:
     - a project linked to the GitHub.com trusted build system;
     - a release signing policy and a test signing policy;
     - one artifact configuration for a zip whose `*.exe` and `*.dll` at its root are all signed, for
       example `<zip-file><pe-file path="*.exe" max-matches="unbounded"><authenticode-sign/></pe-file>`
       and the same for `*.dll` with `min-matches="0"`;
     - two CI users' API tokens, one per policy, each with submitter rights on its own policy only:
       the release token can't sign with the test policy, and the test token can't sign with the
       release policy. Never one token shared by both.
  3. In GitHub:
     - the variables `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`,
       `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG`, `SIGNPATH_RELEASE_POLICY_SLUG` (used for `v*`) and
       `SIGNPATH_TEST_POLICY_SLUG` (used for `test-v*`), at the repository or on the environments;
     - the variable `SIGNPATH_EXPECTED_SIGNER` (above);
     - the secret `SIGNPATH_API_TOKEN` as an environment secret only: the release policy's token on
       `release`, the test policy's on `release-test`. Never as a repository or organization
       secret: GitHub falls back to one of those for an environment without its own, and a workflow
       can't tell the two apart. The workflow reads the token only in `sign-windows` (whose
       environment is the mode's), only when `signing` is `signpath`, and fails unless the job
       runs in the mode's environment.
     `sign-windows` fails early, naming what is missing, when the switch is on without them.
  4. A self-signed dry run, then a `test-v*` run with the test policy, and only then
     `SIGNPATH_ENABLED` for releases.
- **Not proven until such a run.** The steps have not yet run on a Windows runner with signing on:
  - the toolset path;
  - `Set-AuthenticodeSignature` with an untrusted certificate;
  - 7-Zip naming the uninstaller `uninstall.exe`, and its exit code on a signed installer (the
    sandbox check's `7z` pipes treat any non-zero code as a failure);
  - whether the two builds of an uninstaller come out byte for byte the same;
  - `Get-AuthenticodeSignature`'s status for the throwaway certificate (`UnknownError` or
    `NotTrusted` is assumed) and, until a `test-v*` run, for SignPath's (`Valid`);
  - that SignPath pads a file with zeros before its certificate table, as the digest check assumes;
  - that a matrix job's legs each set their own output without clearing the others'.
  The self-signed dry run is the test of each.

## Status

proposed

## Date

2026-11-02
