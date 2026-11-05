# 0037: The updater: the plugin, the host's state machine, the wire and the key

## Context

An installed Clauseo has to find, check and install the next release on its own, and it must
never do so in the middle of an answer, behind a firm's policy, or on an install IT manages.
`tauri-plugin-updater` 2.14.0 is already in the fixed plugin set (`xtask/src/checks/plugins.rs`),
registered Rust-only outside Linux with an empty `pubkey` and no endpoints. HANDOFF item 18 asks
for a decision record before any code uses it, since the plugin set is fixed and this is the one
plugin that downloads and runs code. 0035 (release builds) already signs the updater files and
writes `latest.json`; this record covers the client.

Three constraints shape it:

- The host knows whether a turn is running; the plugin lives in desktop and needs Tauri's async
  runtime, which the host's `LocalPool` does not drive.
- The plugin's Windows `install` ends in `std::process::exit(0)`. If the host called it, desktop
  would never save the counts or the window state, and the host log would not be flushed.
- The webview is untrusted (0001): it may ask for a check or an install, never reach the plugin.

R23 lands in five pull requests, then a one-line sixth with Rohan's real key and the endpoint.
What PRs 3 to 6 deliver is marked below as "lands with PR n";
PR 2 (this record's) adds the wire, stubbed, and `update.failed`.

## Decision

### The plugin's use

- `tauri-plugin-updater` stays registered from Rust only (`plugin_rust_only`), outside Linux, and
  is called only through desktop's adapter. No capability grants an `updater:` permission, so the
  webview has no updater command of its own: `capabilities/main.json` and `settings.json` stay
  without one, and done-when 4 of R23 checks it.
- `plugins.updater` in `tauri.conf.json`: `pubkey`, `endpoints:
  ["https://github.com/clauseo-ai/clauseo-acp/releases/latest/download/latest.json"]`,
  `requireSignedVersion: true`, `allowDowngrades: false`, `windows: { installMode: "passive" }`.
  Full installers, no deltas. `releases/latest` sees only published, non-prerelease releases, so
  neither a draft nor a test pre-release is ever offered. The flags land with PR 5; the pubkey
  and the endpoint land with PR 6.
- **Empty endpoints until the real key.** PRs 1 to 5 land with `endpoints` empty (the
  `tauri-config` check refuses endpoints without a pubkey, and the plugin's `build` fails with
  `EmptyEndpoints`). A build with no endpoint uses `NoUpdater`, shown as `managed` by `package`, so
  it never touches the network. The `e2e` build alone sets an endpoint in code
  (`E2E_UPDATE_ENDPOINT`) and trusts the test key (lands with PR 5).

### The `Updater` trait

The host reaches the plugin through a trait, as it reaches processes through
`process::Launcher` (lands with PR 3, in a new `update` module of `clauseo-host`):

- `install_kind()`: `CurrentUser`, `PerMachine` (the exe under Program Files, or its uninstall
  key in HKLM), `ReadOnlyLocation` (a macOS `.app` whose folder the user cannot write, so the
  plugin would ask for an administrator) or `Package` (Linux, and `NoUpdater`).
- `check()` and `download(offer, progress)` run in the host's state machine. The download
  verifies the minisign signature and the signed version (the plugin's `Update::download`) and
  goes into `<data_dir>/updates/<version>/`, never a matter folder.
- `install` is **not** the host's. On install the host shuts down cleanly and its join returns
  `Exit::Install(downloaded)`; desktop then calls the plugin's `install`, which verifies the
  bytes again (lands with PR 4 for the host, PR 5 for desktop).
- The plugin's futures run on `tauri::async_runtime` and hand back a
  `futures::channel::oneshot`; dropping the receiver aborts the task. Timeouts come from the
  host's `Clock::after`, so `ManualClock` tests drive them. Progress reaches the actor through a
  bounded channel and is never awaited there.
- `HostConfig::with_updater` takes it; the default is `NoUpdater`, so tests and `cli` never touch
  the network.

The state machine is pure, `step(state, input, now) -> (state, effects)` in the
host's `update::machine` module, with time and a seeded jitter as inputs: the first check 1 to
10 minutes after start, then every 60 minutes ± 10. Its transitions are R23's table: `idle` or
`failed` to `checking` on the timer or `CheckForUpdate`; `checking` to `idle` (nothing offered)
or `downloading`; `downloading` to `ready`; `ready` to `installing` (`now`, after stopping turns)
or `scheduled` (`after_turn` while a turn runs); `scheduled` to `installing` when the last turn
ends, or back to `ready` on `CancelScheduledUpdate`; an error in `checking`, `downloading` or
`installing` to `failed`; anything to `managed` when policy or the install kind says so (lands
with PR 3).

### The wire

All additive under 0002 (no `schema_version` change; none of it is logged). In
`crates/protocol/src/update.rs`:

- `UpdateView { current, state, mode, checked_at?, updated_from? }`.
- `UpdateState`: `idle`, `checking`, `downloading { version, received, total? }`,
  `ready { version }`, `scheduled { version }`, `installing { version }`,
  `failed { version?, at, stage: check | download | install }`,
  `managed { by: policy | per_machine | read_only_location | package }`. Sizes are `Count`, the
  crate's safe 64-bit number, so TypeScript reads them exactly.
- `HostQuery::GetUpdate` → `QueryResult::Update(UpdateView)`, read after subscribing to the app
  stream, as `GetSettings` is.
- `HostEvent::UpdateChanged(UpdateView)`, app-wide like `SettingsChanged` (0019's app stream).
- `HostCommand::CheckForUpdate {}`, `InstallUpdate { when: now | after_turn }`,
  `CancelScheduledUpdate {}`, `SetUpdateMode { mode: auto | notify }`. The two without fields are
  empty objects (`{"check_for_update": {}}`), as `InstallBrowser {}` is, so every command stays an
  object and a window tells them apart with `in`. Each answers `Done`; a command
  in a state that does not take it gets `command.invalid`. `validate()` refuses
  `SetUpdateMode { mode: none }` as `command.invalid`, since `none` comes only from policy. The
  `InstallUpdate` receipt goes out before the host enters `Stopping`, and the windows treat the
  drops after it as expected.
- `ErrorCode::UpdateFailed` = `update.failed`, retryable, appended before `unknown` (0003). Its
  English line is "The update didn't install. Try again." (provisional, Hindi later, in Rohan's
  batch review).

In PR 2 the host answers the four commands with `command.invalid` ("the updater is not wired
yet") and `GetUpdate` with `managed` by `package` in `auto`, which is what `NoUpdater` will say.

### The windows' allow-lists

`Bridge::admit` admits the updater by name, each command in its own arm (0019):

| Window | Admitted | Refused |
|---|---|---|
| `main` | `InstallUpdate`, `CancelScheduledUpdate`, `CheckForUpdate`, `GetUpdate` | `SetUpdateMode` |
| `settings` | `SetUpdateMode`, `CheckForUpdate`, `GetUpdate` | `InstallUpdate`, `CancelScheduledUpdate` |
| any other | nothing | everything |

`UpdateChanged` goes to both windows' app feeds; the settings feed restarts with `GetUpdate` as it
does with `GetSettings` (lands with PR 5; until then the main window alone carries it).

### `update.mode` and failing closed

`update.mode` is a settings key in a new `update` section: `auto` (default; download in the
background, install at the next start or on "Restart to update"), `notify` (download, install
only on the lawyer's command) or `none` (managed). The settings window offers only `auto` and
`notify`. `update.mode` is in `SECURITY_KEYS` with safe value `none` through `refuse`, so an
unreadable policy source makes the run `managed` by `policy` and no check runs. So does a policy
value for `update.mode` that does not type, or an `update.*` policy name this version does not
know: the shared rule for a misnamed security key (`typed_policy` in the settings crate) locks
`update.mode` at `none`, since what the firm meant is unknown. Firm IT should add a new
`update.*` policy only after the version that knows it is rolled out. `SetUpdateMode`
on a policy-locked key gives `settings.locked` (lands with PR 1 for settings, PR 4 for the host).

### The install, in order

The install takes the quit path:

1. A running turn under `when = now` goes through Stop, so the descendant sweep runs; its
   turn-end checkpoint finishes (shutdown already waits for the matter jobs). Under `after_turn`
   new sends stay queued and stored, never dropped; cancelling the schedule releases them.
2. The host shuts down: imports and scans cancelled, search, snapshots and matter workers
   stopped, the store closed. Its exit is `Exit::Install(downloaded)`.
3. Desktop's stopper thread flushes the host log (0047) right after `host_start::stop`.
4. Desktop saves the counts and the window state.
5. The "installing <version>" marker (`{ from, to }`) is written to `update.json`
   (`Locations::update_marker()`), off the actor and before the install.
6. Desktop calls the plugin's `install`. Windows exits inside it; macOS swaps the `.app` and
   restarts.
7. If `install` returns an error after the shutdown, desktop relaunches normally. The marker and
   the unchanged version make `read_update_marker` report `failed` (stage `install`) and count it
   at the next start; if NSIS dies after Windows exited, the next launch of the old exe does the
   same. With `to` equal to the running version it reports `updated_from` instead.

Steps 1 and 2, and `read_update_marker`, land with PR 4; steps 3 to 7 with PR 5. S22's crash `Guard::clean_exit`
call before `install` stays a no-op seam until S22 merges.

### The key's custody

- **Rohan's real key.** Rohan runs `tauri signer generate -w clauseo-updater.key` on his MacBook
  with the network off (not the shared Mac mini, which cannot go offline), with a strong password.
  The public key goes into `plugins.updater.pubkey`, committed. He loads the private key and its
  password as the two `release` environment secrets (0035's names) with `gh secret set`, then
  deletes the file.
- **Two offline backups,** both held by Rohan until he names a second holder: his password
  manager, and an encrypted USB stick with the password kept apart.
- **Rotation.** The plugin takes one pubkey. Release N, signed by the old key, carries the new
  pubkey and moves its endpoint to `latest-k2.json`. For 6 months every later release also
  uploads an old-key `latest.json` that points at N, so a client older than N still reaches it.
- **The test key.** A throwaway key, `TAURI_SIGNING_PRIVATE_KEY_TEST` in the `release-test`
  environment, signs test releases (0035's `test` mode); its public half is committed as
  `updater-test.pub` in desktop's release folder (lands with PR 5). Only the `e2e` build trusts it,
  never a release build.

### Deviations from the R23 brief

- **The install order.** The brief has the host call `Updater::install`. Install moves to
  desktop, after `stop_host`, the log flush and the counts and window saves, because the Windows
  install exits the process; the host keeps `install_kind`, `check` and `download` and exits with
  `Exit::Install`.
- **`refuse` and `SECURITY_KEYS`** already exist from R21, so the brief's "until R17, read the
  unreadable flag yourself" step is dropped.
- **S21 is already merged:** the counts events `Update` and `AppOpened { launch: after_update }`
  exist, and the host counts them (lands with PR 4), rather than a no-op until S21.
- **`received` and `total` are `Count`,** not bare `u64`, by the protocol's rule for 64-bit
  numbers.

## Consequences

- The webview can start a check or an install only through the host's commands, which the bridge
  admits by window; it never reaches the plugin or the network.
- Every exhaustive match on `HostCommand`, `HostQuery` and `HostEvent` gains arms; open pull
  requests with such matches must add them before they merge.
- The host never depends on the plugin, Tauri or tokio; a test drives the whole state machine
  with a fake `Updater` and `ManualClock`.
- Until PR 6, no build updates itself: every build reads `managed`.
- A later move of the endpoint (a Worker, staged rollout) changes only the config and the
  adapter; the wire and the state machine stay.

## Status

proposed

## Date

2026-11-05
