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
  neither a draft nor a test pre-release is ever offered. The flags landed with PR 5; the
  pubkey and the endpoint landed with PR 6.
- **Empty endpoints until the real key.** PRs 1 to 5 land with `endpoints` empty (the
  `tauri-config` check refuses endpoints without a pubkey, and the plugin's `build` fails with
  `EmptyEndpoints`). A build with no endpoint uses `NoUpdater`, shown as `managed` by `package`, so
  it never touches the network. The `e2e` build alone sets an endpoint in code
  (`E2E_UPDATE_ENDPOINT`) and trusts the test key (landed with PR 5).
- **PR 6: the endpoint, built against a placeholder key.** PR 6 sets `endpoints` to the URL above
  and `pubkey` to a placeholder: a throwaway minisign key made for PR 6 alone, whose private
  half and password were deleted at once, so nothing can sign for it. Its public half is
  committed as `crates/desktop/release/updater-placeholder.pub`. While it is the release key,
  the desktop test `the_release_pubkey_is_not_the_placeholder` fails ("PR 6 can't merge until
  Rohan's public key replaces the placeholder"), so PR 6 cannot merge by accident; a release
  run would fail too, since the bundler checks the signing key against that pubkey. A release
  build now gives the host `PluginUpdater` over the config; a debug build without `e2e`
  still gives none.
- **The swap, when Rohan's key arrives** (R23-key-steps.md): paste his `.pub` text as
  `plugins.updater.pubkey` in `tauri.conf.json`; delete `updater-placeholder.pub` and the
  test that reads it (`the_committed_placeholder_has_the_key_id_the_guard_refuses`). The guard
  stays and keeps the placeholder's key id in the test (`PLACEHOLDER_KEY_ID`), so the placeholder
  can never come back; it also refuses the test key, and a pubkey that is not a minisign public
  key. Keeping it costs nothing, and deleting it would leave only "not empty" to catch a wrong
  key.

### The `Updater` trait

The host reaches the plugin through a trait, as it reaches processes through
`process::Launcher` (lands with PR 3, in a new `update` module of `clauseo-host`):

- `install_kind()`: `CurrentUser`, `PerMachine` (the exe under Program Files, or its uninstall
  key in HKLM), `ReadOnlyLocation` (a macOS `.app` whose folder the user cannot write, so the
  plugin would ask for an administrator) or `Package` (Linux, and `NoUpdater`).
- `check()` and `download(offer, progress)` run in the host's state machine. The download
  verifies the minisign signature and the signed version (the plugin's `Update::download`) and
  goes into `<data_dir>/updates/<version>/`, never a matter folder. Both return
  `BoxFuture<'static, _>`, so the actor's pool runs them without borrowing the updater.
- The values: `Offer { version, handle }` and `Downloaded { version, path, handle }`, where
  `handle` is an `Opaque` (a shared `Any`) holding whatever desktop needs (the plugin's
  `Update`), which the host only moves; `UpdaterError` is `Network`, `Signature`, `Io` or
  `Other`, each with a log line, and any of them fails the stage that was running.
  `ProgressSink::post(received, total)` takes a running total and drops the value when the
  bounded channel is full.
- `install` is **not** the host's. On install the host shuts down cleanly and
  `HostHandle::exit` resolves `HostExit::Install(downloaded)` once its thread has stopped;
  desktop then calls the plugin's `install`, which verifies the bytes again (landed with PR 4
  for the host and PR 5 for desktop).
- The plugin's futures run on `tauri::async_runtime` and hand back a
  `futures::channel::oneshot`; dropping the receiver aborts the task. Timeouts come from the
  host's `Clock::after`, so `ManualClock` tests drive them. Progress reaches the actor through a
  bounded channel and is never awaited there.
- `HostConfig::with_updater` takes it; the default is `NoUpdater`, so tests and `cli` never touch
  the network.

The state machine is pure, `step(state, input, now) -> Result<(state, effects), Refusal>` in
the host's `update::machine` module, with time and a seeded jitter (SplitMix64, seeded by the
caller) as inputs: the first check 1 to 10 minutes after start, then every 60 minutes ± 10.
`Machine::start` builds the first state from what start-up read (the install kind, the mode in
force and its lock, `update.json`, last run's download and the seed), so no machine exists
before start-up. The inputs: the timer, `CheckRequested`, `CheckDone`, `Progress`,
`DownloadDone`, `InstallRequested(when)`, `CancelScheduled`, `ModeSet`, `PolicyChanged`,
`TurnsRunning(bool)` (each time "any turn running" flips) and `TurnsCancelled`. The effects, in
order: `StartCheck`, `StartDownload`, `CancelDownload`, `ArmTimer { at }` (one timer, each
replacing the last), `CancelTurns`, `GateNewTurns(bool)`, `WriteMarker`, `ClearMarker`,
`BeginInstall(downloaded)`, `Publish` (whenever the view changed) and `Count(result)`. A
command in a state that does not take it is `Refusal::NotNow` (`command.invalid`);
`SetUpdateMode` while policy locks `update.mode` is `Refusal::Locked` (`settings.locked`).
An event a state no longer concerns (a check's result after the run became managed, a replaced
timer) changes nothing. Its transitions are R23's table: `idle` or
`failed` to `checking` on the timer or `CheckForUpdate`; `checking` to `idle` (nothing offered)
or `downloading`; `downloading` to `ready`; `ready` to `installing` (`now`, after stopping turns)
or `scheduled` (`after_turn` while a turn runs); `scheduled` to `installing` when the last turn
ends, or as `ready` with `now` on `InstallUpdate { when: now }` ("Restart now" upgrades the
wait in place), or back to `ready` on `CancelScheduledUpdate`, which also takes back a `now` install still
waiting for its cancelled turns and releases the gate; an error in `checking` or `downloading`
to `failed`, and `installing` to `failed` at the next start through the marker; anything to
`managed` when policy or the install kind says so, except an install whose `BeginInstall` has
gone out (lands with PR 3). Further rules:

- **Managed** is decided policy first (`none` in force gives `managed` by `policy`), then the
  install kind. A managed run never emits `StartCheck`, and counts `managed` at most once a
  day, at start and on a 24-hour timer.
- **`failed` counts** always for an install failure (the marker) and for a check or download
  the lawyer asked for (`CheckForUpdate`), and at most once a day for background ones, so an
  offline laptop counts one failure a day rather than one an hour.
- **The timer** is one-shot in the actor and its deadline is wall-clock time: a fire before the
  deadline (the clock stepped back) arms it again at the same deadline rather than dropping it.
- **`now` and `after_turn`.** `now` with turns running gates new sends and cancels the turns,
  and installs on `TurnsCancelled`. `after_turn` does not gate: new turns start, and the
  install waits until no turn runs (`TurnsRunning(false)`), then counts `after_answer`.
- **The marker.** Just before `BeginInstall` the machine has the marker
  `{ from, to, state: "installing" }` written. At start, a marker with `to` equal to the running
  version sets `updated_from` and counts `installed`; with `from` equal it is `failed` (stage
  `install`) and counts `failed`. Any marker is then cleared, so it counts once.
- **Last run's download.** In `auto`, start-up checks at once; an offer of the same version
  installs from the file (`scheduled` while a turn runs). Offline, the check fails as any check
  does and nothing installs; a later offer of that version is `ready` with no second download,
  and installs on the lawyer's command or at the next start. In `notify` the file only saves
  the download, and so it does once the mode in force leaves `auto` while start-up's check runs. After a failed install the file is never reused.

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

In PR 2 the host answered the four commands with `command.invalid` ("the updater is not wired
yet") and `GetUpdate` with `managed` by `package` in `auto`, which is what `NoUpdater` says.
PR 4 wires them (below, "The host's wiring").

### The windows' allow-lists

`Bridge::admit` admits the updater by name, each command in its own arm (0019):

| Window | Admitted | Refused |
|---|---|---|
| `main` | `InstallUpdate`, `CancelScheduledUpdate`, `CheckForUpdate`, `GetUpdate` | `SetUpdateMode` |
| `settings` | `SetUpdateMode`, `CheckForUpdate`, `GetUpdate` | `InstallUpdate`, `CancelScheduledUpdate` |
| any other | nothing | everything |

`UpdateChanged` goes to both windows' app feeds; the settings feed restarts with `GetUpdate` as it
does with `GetSettings` (landed with PR 5).

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
   turn-end checkpoint finishes (shutdown already waits for the matter jobs). Meanwhile new sends
   stay queued and stored, never dropped; cancelling the install releases them. Under
   `after_turn` nothing is held: new turns run, and the install waits for the last to end.
2. The "installing <version>" marker (`{ from, to }`) is written to `update.json`
   (`Locations::update_marker()`), off the actor; the shutdown starts only once the write has
   ended on disk, new sends held meanwhile. A write that fails installs nothing (a failed
   install would then report nothing at the next start): the run shows `failed` at the
   `install` stage, counts it, and new sends run again. A quit that overtakes the install while the
   marker is written installs nothing either: the marker is deleted again, on the same thread
   behind the write, before the host stops.
3. The host shuts down: imports and scans cancelled, search, snapshots and matter workers
   stopped, the store closed. Its exit is `HostExit::Install(downloaded)`
   (`HostHandle::exit`), only when the store closed cleanly; otherwise the marker is deleted
   (off the actor, waited for) and the exit is `Stopped`: desktop just quits, and the next start
   finds the download again and reports no failed install.
4. Desktop's stopper thread flushes the host log (0047) right after `host_start::stop`.
5. Desktop saves the counts and the window state.
6. Desktop calls the plugin's `install`, unless an agents job outlived its bound at quit (then it
   relaunches as the old version, as in 7). Windows exits inside it; macOS swaps the `.app` and
   restarts. One that has not returned in 5 minutes is given up on (the install's watchdog,
   below).
7. If `install` returns an error after the shutdown, or was skipped or given up on, desktop
   relaunches normally. The marker and
   the unchanged version make `read_update_marker` report `failed` (stage `install`) and count it
   at the next start; if NSIS dies after Windows exited, the next launch of the old exe does the
   same. With `to` equal to the running version it reports `updated_from` instead.

Steps 1 to 3, and `read_update_marker`, landed with PR 4; steps 4 to 7 landed with PR 5. S22's crash `Guard::clean_exit`
call before `install` stays a no-op seam until S22 merges.

### The host's wiring (PR 4)

`crates/host/src/actor/update.rs` runs the machine; what the R23 brief left open was decided so:

- **Start.** Start-up reads `update.json` and last run's download on the `clauseo-update` thread
  and waits for it; the machine starts once the host serves, so an install at start in `auto`
  never stops a host before `Host::start` has answered. Every later touch of `update.json`
  (the clear, the marker) runs on that thread in order; the exit waits for the marker.
- **Last run's download** is the newest `updates/<version>/` folder, of a dotted-number version
  newer than the running one, holding exactly one file; PR 5's adapter must save it so. A file
  the download left half written passes this test; the plugin's `install` verifies the bytes
  and signature again and refuses it. Start-up removes everything else in `updates/`, and that
  download too when the marker says an install failed (`from` the running version), whatever
  its version, as the machine drops it then.
- **The marker** is written through a temporary file, synced, renamed, then the data folder is
  synced, so the rename survives a power cut during the install. A failed folder sync is logged
  and the install goes ahead: the marker is on disk, and reporting a failed write would count a
  failure the next start counts again.
- **Commands.** `CheckForUpdate`, `InstallUpdate` and `CancelScheduledUpdate` answer `Done` with
  no receipt, as `InstallBrowser` does: their state lives in memory. `InstallUpdate`'s answer
  goes out before the host starts to stop. `SetUpdateMode` is checked by the machine, written to
  `settings.json`, then handed to the machine as the mode the settings hold; it keeps a receipt,
  counts `setting_changed` and sends `SettingsChanged` as the other settings writes do.
- **"A turn is running"**, for `after_turn`: a thread has a current turn, a last `TurnEnded` not
  yet durable, or queued turns that start by themselves. A held queue does not count: it waits
  for the lawyer and would keep the install waiting for as long as the lawyer leaves it.
- **The hold under `now`.** Every current turn goes through Stop's path. A send while turns stop
  is stored queued, whatever its delivery, behind its own thread's turn or alone; none starts.
  When the install is taken back, each queue moves on as it would have. The machine always
  hears `TurnsCancelled`, also when no turn was left to cancel.
  It is one of a thread's hold reasons with the sign-in's and `stop_agents`' (0048): one held
  append, each lift removing only its own reason, nothing starting until none is left; with a
  sign-in's hold too, the prompt queues as the sign-in's does. Under it a steer no longer joins the running turn,
  which the install is stopping: it queues as a send now does. The most specific reason held a
  prompt goes in the host log only; the wire has `queue_held` alone.
- **Timers.** One timer on the host clock; a fire the machine finds early re-arms.
- **The jitter seed** comes from the clock at start; `HostConfig::with_update_jitter_seed` fixes
  it for tests, and `with_app_version` gives the app's version (desktop passes its own).
- **`UpdateChanged`** is not among the app feed's first frames: a window asks `GetUpdate`.
- **Counts.** `managed` counts once a day across starts: the actor skips it when the open day's
  counts already hold it, with nothing new saved. A background `failed` counts once a day per run
  (the machine's rule), since it shares its key with the failures the lawyer asked for, which
  always count, as does a failed install.
- **Logs** name a failed step's stage and kind (`network`, `signature`, `io`, `other`), never its
  text, which may hold a path or a URL (0047).

### Desktop's adapter (PR 5)

`crates/desktop/src/update.rs` and its modules implement the trait over the plugin and run the
install; what the brief left open was decided so:

- **Which updater.** Linux and debug builds give the host none (`NoUpdater`, `managed` by
  `package`), and so would a release build whose `tauri.conf.json` named no endpoint (as before
  PR 6), so the plugin's `build` (which fails with `EmptyEndpoints`) is never called. Since PR 6
  a release build takes the plugin over the config's endpoint and key. The `e2e` build takes
  an updater only from its variables, never from the config, so a test run never reaches the
  real endpoint: `E2E_FAKE_UPDATE=<version>` (a fake, installed per user, that offers that
  version at once, finishes its download at once with no file, and is never installed: the app
  just exits), else `E2E_UPDATE_ENDPOINT` with the test key, else none. The test key is
  compiled into the `e2e` build alone (`TRUSTS_TEST_KEY`). Three layers keep it there. A
  desktop test fails if any file of the crate (tests aside) holds the key in any form or a piece
  of its path, except the key's file and the one `include_str!` line in `choice.rs`, directly
  under `#[cfg(feature = "e2e")]`. A second fails if the non-`e2e` test binary holds the key,
  but the linker drops code only `run()` reaches, so that covers only what the tests reach. The
  real control is `release.yml`'s scan of each shipped app binary before it is signed or
  uploaded (0035, "No test key in a shipped binary"), which fails a release or dry run that
  holds the key. The same scan fails on the `e2e` build's variable names (`E2E_UPDATE_ENDPOINT`,
  `E2E_FAKE_UPDATE`) and on its marker, `CLAUSEO-E2E-BUILD-MARKER`, so neither the test release's
  endpoint nor the fake can reach a shipped binary.
- **The plugin** is reached through `UpdaterBuilder`, with a 30 s connect and a 60 s read
  timeout (the host sets no deadline on a check or a download), and HTTPS only
  (`configure_client` sets reqwest's `https_only`, in every build, `e2e` included, whose test
  release is on GitHub over HTTPS): the check and the download refuse an `http://` URL and a
  redirect from HTTPS to HTTP before connecting. The plugin alone refuses an `http://` endpoint
  only in a release build, only warns for an `http://` download URL, and checks no redirect. A release that has no installer
  for this platform (`TargetNotFound`) is no update, not a failure. An error reaches the host
  as a fixed line by its kind, never the plugin's text, which can name the URL or a path.
- **The download** is written to a temporary file of its own, `updates/.<version>.<uuid>.part`
  (a new UUID v7 each time, so two downloads of one version never write the same file), synced, then moved into an emptied
  `updates/<version>/` as its one file, so that folder never holds a half-written download.
  A version that is not plain dotted numbers is refused. A download the host cancels after its
  bytes arrived can still be saved (the write runs off the cancelled task); that is harmless,
  since the plugin verified those bytes and the install verifies them again. A temporary file
  a crash left is removed at the next start, which keeps nothing in `updates/` but the folder it
  offers.
- **Install kind.** Windows: per machine when `Clauseo.exe` is under Program Files or the
  machine's uninstall key `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo`
  (64-bit view) exists; a key that cannot be read counts as there. macOS: a read-only location
  when the folder holding `Clauseo.app` or the `.app` itself fails `access(W_OK)` (moving the
  old bundle out needs both, so a root-owned `.app` from an MDM would ask for an
  administrator), or the executable is not in an `.app`. Found once at start.
- **The install, as built.** A task watches `HostHandle::exit`; `HostExit::Install` asks the app
  to exit, which runs the stopper thread (`quit::stop_then_after_host`, its order tested): the
  quit's own order (the agents' jobs, then `host_start::stop`), the log's flush, the counts'
  save, then the window state's save (the install and the restart skip the exit event
  window-state saves on), the crash guard's no-op seam, a second flush, and the plugin's
  `install`. When an agents job outlived its 3 s bound at quit (`Agents::shutdown` answered
  false), nothing is installed: a child of it may still hold the app's files open. The window
  state is saved and the app restarts as the old version, so the next start's marker reads
  `failed`; the log says why at warn, with `kind = "agents"`. It rebuilds the update with `restore_update(raw_json)`, checks the version is the
  downloaded one, and hands it the file's bytes, which it verifies again. Windows exits inside
  it (`installMode: passive`); after it returns, whether it worked (macOS) or failed, the app
  restarts. A plain exit saves the window state on the exit event, as before. The host log
  keeps the install on the `clauseo_desktop::lifecycle` target ("installing the update" and
  "the update is installed", with the version), and a failure at warn with its kind and fixed
  line (an old app the plugin could not put back is its own I/O line).
- **The install's watchdog.** The plugin's `install` runs on a thread of its own; the stopper
  waits 5 minutes for it to return (macOS) or the process to exit inside it (Windows), then
  gives up, logs `kind = "hung"` and restarts, so the next start's marker reads `failed` (or
  `updated_from`, if the new app was in place by then). The bound covers only the work inside
  this process. On Windows that is verifying the bytes, writing the installer to a temporary
  file and starting it: the NSIS installer, however slow in passive mode on a slow disk, runs
  after the exit in its own process, which no bound here can cut short. On macOS it is
  verifying, unpacking the `.app` on the app's volume (seconds for a few hundred MB, under a
  minute on a slow disk), the swap, and the administrator's password prompt when the folder
  needs one; 5 minutes is several times the slowest unpack and leaves a person time to answer
  the prompt. The swap cannot be cut mid-move: on APFS the plugin swaps the bundles with one
  `renamex_np(RENAME_SWAP)`, a single system call a process exit cannot split. Only on a file
  system that cannot swap (HFS+) does it make two renames, the old app out and then the new one
  in, microseconds apart; past the bound the watchdog first checks the app's executable is at
  its path, and while it is not, it waits up to 60 s more (a look each second) for the install
  instead of restarting. Giving up anywhere else leaves the old app or the new one in place,
  and the plugin's temporary folders as litter macOS clears. The one gap left is the instant
  between that check and the restart: were the second rename lost there, the old app stays in
  the temporary folder and the person reinstalls from the download page. The decision is
  `install::watch_install`, tested with an injected slow install.

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
  `updater-test.pub` in desktop's release folder (landed with PR 5). Only the `e2e` build trusts it,
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
- **The marker is written before the shutdown,** not after it: the machine emits it with
  `BeginInstall`, so a shutdown that never reaches the install still reads `failed` at the
  next start rather than nothing.
- **A stale marker is cleared** once start-up has read it, so `installed` or `failed` counts
  once; the brief does not say.
- **`received` and `total` are `Count`,** not bare `u64`, by the protocol's rule for 64-bit
  numbers.

## Consequences

- The webview can start a check or an install only through the host's commands, which the bridge
  admits by window; it never reaches the plugin or the network.
- Every exhaustive match on `HostCommand`, `HostQuery` and `HostEvent` gains arms; open pull
  requests with such matches must add them before they merge.
- The host never depends on the plugin, Tauri or tokio; a test drives the whole state machine
  with a fake `Updater` and `ManualClock`.
- Until PR 6, no build updated itself: every build read `managed`. Since PR 6, per-user release
  builds on Windows and macOS check the release endpoint.
- A later move of the endpoint (a Worker, staged rollout) changes only the config and the
  adapter; the wire and the state machine stay.

## Status

proposed

## Date

2026-11-05
