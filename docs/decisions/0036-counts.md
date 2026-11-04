# 0036: The usage counts crate and its wire format

## Context

Research 37 (approved 2026-10-31) settles what Clauseo may count: 18 events, each a few closed
fields, added up per UTC day on the lawyer's machine, plus a weekly tally of crashes. Two things
must hold where a reviewer can check them. No document text, path, name or error message can
reach a payload, and nothing is saved or sent before the lawyer's first act or after they (or
their firm's policy) turn counts off. S23's Cloudflare Worker must read the payloads with the same
rules the app writes them by, and it builds for `wasm32`.

## Decision

A new crate, `crates/counts` (`clauseo-counts`), holds the schema, the install id, the day's
cells, the weekly tally, the queue and the sender. Its row bans `matters`, `acp`, `reducer`,
`store`, `host`, `tauri`, `tokio` and `reqwest`.

- **Two halves.** The `schema` module does no I/O and reads no clock. The files, the ids from the
  OS random source and the sender sit behind the default feature `client`. The Worker builds
  `default-features = false` for `wasm32-unknown-unknown`.
- **Closed at compile time.** Every payload field implements the sealed trait `Closed`: the
  crate's enums, its capped integers, buckets, days, weeks, versions, the two random ids,
  `StrictErrorCode`, and structs and lists of those. `String`, `PathBuf` and bare integers do
  not. A `trybuild` test shows a `String` field refused.
- **Closed on the wire.** Every struct refuses an unknown field, every enum an unknown name, every
  number a value outside its cap, and every id, day, week or version any form but one. A committed
  JSON Schema snapshot is walked by a test that fails on a string with no fixed values (UUID v4,
  day, week and version patterns aside) or an integer with no maximum.
- **`error_shown.code` is `StrictErrorCode`.** `ErrorCode`'s own `Deserialize` turns an unknown
  string into `Unknown`; the newtype reads only through `ErrorCode::from_str` and refuses
  `unknown` too.
- **The wire format.** `POST /v1/counts` takes `{ "days": [ … ] }`, 1 to 7 closed days, oldest
  first, gzip at most 16,000 bytes and 64,000 bytes of JSON. Each day is its envelope plus
  `"cells": { "<event>": [ { "<field>": <value>, …, "n": <1..9999> } ] }`, with only non-zero
  cells and no event without one. `setting_changed` cells carry `setting` and `value`, the value
  in that setting's own enum. `POST /v1/tally` takes one tally (gzip at most 4,000 bytes and
  16,000 of JSON). It has no install id and no language, and has its own random `tally` id, so
  a crash cannot be joined to an install. A day goes at most 14 days after it closes, and a tally
  at most 4 weeks after its week.

  ```json
  { "days": [ {
      "schema": 1, "install": "8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40", "day": "2026-11-01",
      "version": "3.0.4", "channel": "stable", "os": "macos", "os_major": 15,
      "arch": "aarch64", "language": "en",
      "cells": {
        "app_opened": [ { "launch": "cold", "n": 2 } ],
        "time_open": [ { "bucket": "lt_3h", "n": 1 } ],
        "question_sent": [ { "agent": "claude", "delivery": "now", "n": 14 } ],
        "setting_changed": [ { "setting": "text_size", "value": 18, "n": 1 } ],
        "error_shown": [ { "code": "agent.exited", "n": 1 } ]
      } } ] }
  ```

  ```json
  { "schema": 1, "tally": "1c0e7d52-9a43-4f1b-8e6d-3b2a9f7c4d18", "week": "2026-W44",
    "version": "3.0.4", "channel": "stable", "os": "windows", "os_major": 11,
    "arch": "x86_64", "install_type": "per_machine", "managed": true,
    "cells": {
      "crash": [ { "process": "webview", "kind": "renderer_gone", "after_launch": false, "n": 1 } ],
      "active_week": [ { "n": 1 } ] } }
  ```

- **`answer_ended.first_words` is optional** (decided 2026-11-03). An answer can end before any
  words (stopped at once, a failed launch, offline, a limit), and no duration bucket can say so
  without inventing a time, so the field is left out then. Research 37 §2.2 is amended to match
  (private repo, 3c5e861). The schema stays 1, defined with the field optional: no build has
  ever sent schema 1 (the release endpoint is empty and nothing calls the first act yet), so no
  older client reads it the other way.
- **Files.** `<data_dir>/first_act_done` (written at the first act, never deleted) and
  `<data_dir>/counts/`: `install_id`, `today.json`, `queue/<day>.json`, `tally/<week>.json`. Each
  is written whole through a temp file and a rename, only in `src/write.rs`. A file that does not
  read as its type is removed, and so is a queued day or a tally made under another install id
  (a tally file keeps its id on disk only). A report is never given another id.
- **Modes.** Off (the switch or policy) counts nothing, deletes `<data_dir>/counts/`, and cancels
  a POST in flight, which is never retried. On before the first act counts in memory only. On
  after it saves and sends. Turning counts on again makes a new install id, once the old folder
  is gone: while its delete fails, counts stay off. Post numbers never repeat in a process, so an
  answer to a POST from before off matches nothing after it.
- **The sender.** Time and randomness are inputs: `Counts::tick(now)`, and a seed given to
  `Counts::open`. One day send a UTC day, 0 to 10 minutes after the first tick and after each
  midnight. A failed try is retried 1, 3, 7, 11, 15, 19 and 23 hours after the first; then the days
  wait for the next daily send. A 2xx or a `400` takes the days out of the queue. A tally goes
  alone, at a random moment in the week after its week, never within an hour of a day try. One
  POST is in flight at most, and one with no answer after 10 minutes is a failed try. The network
  is the `Upload` trait, which desktop implements with `reqwest` (tokio stays out of `host`), and
  `send_due` reads the effective switch before every POST.

## Consequences

- A new event, field or value is a schema change: add it to the enum before the release that
  sends it, and to the Worker before that, as for error codes. The snapshot test shows the diff.
- `setting_changed` has no `preparing_ahead` or `keep_versions` yet; their settings do not exist.
  They are added, append-only, with those settings.
- **A rule for the host, in two halves.** (1) Off is recorded in settings (`privacy.counts`
  false, in `settings.json`, outside `counts/`) even when `Counts::set_enabled(false)` fails to
  delete the folder, so the next start opens counts off, sends nothing and tries the delete
  again. (2) While `Counts::set_enabled(true)` returns an error (the old folder still cannot be
  deleted), on is refused: settings stay false. The crate deletes `install_id` first and drops
  `today.json`, queued days and tallies made under another id, which covers an off that failed
  part way; but when `counts/` itself cannot be changed, the old id cannot be removed, and only
  these two halves keep it from being resumed.
- The host and desktop wiring comes in two more pull requests: the settings keys, the wire and
  the host's call sites (below, "The switches and the host"), then desktop's `reqwest` upload,
  its tick task, `count_ui` and the bridge arms (below, "The desktop"). Until UD1's welcome sends
  `FirstAct`, nothing starts counts, so nothing is saved or sent.

## The switches and the host (S21's second pull request)

- **Settings.** A new section `privacy`: `privacy.counts` (default `true`) and
  `privacy.crash_reports` (default `false`, for S22). Both are in `SECURITY_KEYS`, so the
  shared `refuse(key, settings)` (`crates/settings/src/leaf.rs`, from R21) locks each `false`
  when a policy source cannot be read, when a policy value does not type, and when a policy names
  the section in a shape this version cannot read: no second mechanism. A policy can only turn
  them off (`OFF_ONLY_KEYS`): a policy `true` is ignored with a warning in `Effective::warnings`
  and does not lock, so the lawyer's switch stands; a policy `false` locks the key.
- **`Effective::policy_present`** is whether any policy source was there: read (even one silent
  on every key, or `{}`) or unreadable. It is `false` only when every source was absent. It is the
  tally's `managed`; desktop fills `About` with it.
- **The wire** (protocol; no new `ErrorCode`, additive under 0002's freeze rules).
  `SettingsView.privacy: PrivacySettings { counts, crash_reports, counts_started }`, where
  `counts_started` is `Counts::consented` (the marker exists). `HostCommand::SetPrivacySettings {
  patch: PrivacyPatch { counts, crash_reports } }` follows `SetUiSettings`' rules (0017): a patch
  naming a locked key is refused whole with `settings.locked`, and a write sends
  `SettingsChanged`. `HostCommand::FirstAct { counts }` is the welcome's button. Both log only
  their receipt, as every settings command does: the switches live in `settings.json`, not the
  log, and no reducer state changes (`REDUCER_SCHEMA_VERSION` stays).
- **The first act.** Before `<data_dir>/first_act_done` exists, a `SetPrivacySettings` naming
  `counts` is the first act, since the lawyer changed the switch in view: it writes the marker,
  and with `true` starts saving. `FirstAct { counts }` does the same with the switch as the
  lawyer left it; a policy lock on `privacy.counts` stands over it, and only the marker is
  written. Once the marker exists, `FirstAct` changes nothing and answers `Done`. A host given no
  counts (`HostConfig::with_counts` not called: tests, `cli`) saves the switches, and its
  `FirstAct` changes nothing.
- **The rule for the host above, as built** (`crates/host/src/actor/settings/privacy.rs`). Off
  writes `privacy.counts: false` first, then, if it is the first act and only once that write
  succeeded, the marker, then `Counts::set_enabled(false)`; a failed delete is logged and retried
  by each tick and the next start, and the command answers `Done`. When the settings write itself
  fails, counts still stop, but the command is refused (`internal`), no marker is written and
  `privacy.counts` has not changed: the switch keeps showing on, and the stop lasts only this
  session, since the next start reads `true` (and, before a first act, counts in memory only,
  so a refused opt-out never becomes consent). The `clauseo-place` thread's queue holds 8 counts;
  behind a lookup that hangs, the next are dropped. On calls `Counts::set_enabled(true)` first; while it fails (the old folder
  cannot be deleted) the command is refused with `store.io`, counts stay off and
  `privacy.counts` stays `false`. A settings write that fails after on turns counts back off.
  Off is otherwise refused only by a policy lock. Every app subscriber gets `SettingsChanged`
  whenever the view changed, a refused command's included: a first act whose settings write
  failed has still written the marker and started counts.
- **The host's call sites** (`crates/host/src/actor/counting.rs`). Each bump is one lock and one
  increment on the actor; with no counts it is a no-op with a debug log, and it never fails a
  command. Each event counts once per operation that succeeded, when it is durable:
  - `app_opened` (`cold`), when the host has started; `after_update` waits for R23's marker.
  - `matter_opened`, at `CreateMatter`'s commit (`folder` with the import's papers, or
    `new_folder` with none) and at `OpenMatter`'s (`folder`, its papers listed on the matter's
    worker as `ListPapers` lists them, and only while counts are on). `place` is
    `clauseo_matters::place` (0032), read on a `clauseo-place` thread since it stats the folder,
    and counted by its kind alone (`local`, `network`, `cloud_sync`; never the service's name).
    One `clauseo-place` thread serves the process, so a stat a network share holds up strands
    one thread at most and never the actor.
    `protection` is `not_protected` until R17. The Scratch matter is not counted.
  - `question_sent`, at the prompt's commit: `now` (started at once, or sent now over a running
    answer), `queued`, or `steer`. An agent with no value in the schema (`fake-agent`, a firm's
    own) is not counted.
  - `answer_ended`, in `turn::end::finish`, the one place every turn ends, never per stop:
    `EndTurn` is `done`, `Cancelled` `stopped_by_lawyer`, `AtLimit` `limit`, `MaxTokens`,
    `MaxTurnRequests` and `Refusal` `agent_stopped`, a failure with `agent.auth_required`
    `signin_expired`, any other failure `error`. `took` runs from the turn becoming current to its
    end, `first_words` to its first answer text, on the host's monotonic clock. `offline` has no
    signal in the host yet.
  - `feature_used`: `undo` and `restore` once their log is durable.
  - `setting_changed`, after a write, for each `ui` value that changed (`language`, `theme`,
    `text_size`, `lite_mode`, `reduced_motion`) and for `crash_reports`; never for counts' own
    switch.
  - Left to their owners: `limit` (R16's view), `agent_install` and `sign_in` (R20),
    `permission_asked` (R18), `update` (R23), `onboarding_step`, `error_shown`,
    `break_shown` and the webview's `feature_used` values through `count_ui` (desktop).
  - `feature_used.versions` moves to the webview's `count_ui` (decided 2026-11-04): opening
    Versions makes no host call, and `compare_versions` is the Versions panel's Compare, which it
    asks again whenever the matter moves, so counting it in the host would count attempts, not
    uses. `undo` and `restore` stay host-counted, once per success.

## The desktop (S21's third pull request)

- **Opening.** Desktop opens the counts on the host's start thread, before `Host::start`
  (`crates/desktop/src/counts.rs`), with `privacy.counts` after policy (off, too, when the
  user's `settings.json` exists but is unreadable, is not JSON or holds a `privacy` value that
  does not type: `Effective::load` takes the defaults then, and the default would resume counts
  the lawyer may have turned off; the other settings keep their defaults), whether
  `<data_dir>/first_act_done` exists, a seed from the OS random source (`os_seed`) and the
  envelope (`counts/about.rs`): desktop's `CARGO_PKG_VERSION`; `channel` `stable`, a build
  constant; macOS's major version, or on Windows `11` from build 22000 and `10` before it (from
  `tauri-plugin-os`); `ui.language`; `per_machine` when the Windows executable sits under the
  Program Files known folder, else `per_user`; `managed` from `Effective::policy_present`. A
  machine the schema has no value for (an OS version outside it, another CPU) gets no counts that
  run: nothing is counted, saved, sent or deleted. The handle goes to the host through
  `HostConfig::with_counts`.
- **The tick task.** One task on Tauri's runtime ticks every 60 s: `Counts::tick` on a
  blocking thread, then `send_due`. Just before, it asks the host for `privacy.counts` after
  policy (`GetSettings`): the value `SetPrivacySettings` and `FirstAct` switch the counts by,
  answered between commands. A host that did not start, has shut down or does not answer gives
  no value, and that tick switches and sends nothing (never an off: an exit would otherwise
  delete the counts). The
  switch's generation (`Counts::switch_generation`, which `set_enabled` and `first_act` move) is
  taken before the read; an off read while it moved is older than the switch (the host turned
  counts on and its settings write had not landed) and turns nothing off that tick. The host
  reads policy at start, so a policy that appears later takes effect at the next start, as
  everywhere in the app. The exit stops the task before it stops the host, then ticks once
  more, after the host has stopped, to save. The clock is read
  in `counts.rs` alone, under `#[expect]`.
- **`time_open`** is the foreground time of the `main` and `settings` windows (either holding the
  focus), through `Counts::set_foreground`. **`app_opened`** stays the host's (above); desktop
  adds none.
- **The upload** (`counts/upload.rs`): reqwest 0.13 with rustls and ring (the provider
  `tauri-plugin-updater` installs; no second one), the OS's certificate check, HTTPS only, no
  redirect, no cookie store, no `Referer`, a 60 s timeout. A POST names `User-Agent: Clauseo`,
  `Content-Type: application/json` and `Content-Encoding: gzip`; reqwest adds `Accept: */*` and
  hyper `Host` and `Content-Length`, and nothing else goes: a test reads the bytes sent, at a
  server on 127.0.0.1 with HTTPS only relaxed for the test alone, and fails on any other header.
  Only the status of the answer is read.
- **The proxy** is the system's, through reqwest's `system-proxy` (hyper-util's
  `client-proxy-system`, read in hyper-util 0.1.21's `client/proxy/matcher.rs`): the
  `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`/`NO_PROXY` variables first, then on macOS the network
  settings from `SCDynamicStoreCopyProxies` (`HTTPSEnable`, `HTTPSProxy`, `HTTPSPort`, and the
  HTTP ones), and on Windows the per-user Internet Settings (`ProxyEnable`, `ProxyServer`,
  `ProxyOverride`). Not read: macOS's exception list, a Windows `ProxyServer` split per protocol,
  and PAC files (Later, G10). The agents' egress proxy goes to agents in their own environment and
  never into the app's, so the upload cannot take it.
- **The endpoint.** Release builds post to a constant that is empty until a follow-up pull request,
  made after S23's live checks pass, sets `https://t.clauseo.chat`; until then they send nothing.
  Debug and `e2e` builds read `CLAUSEO_COUNTS_URL` instead and have no endpoint without it;
  release builds never read it. An endpoint is an `https` origin with no user, query, fragment or
  path. With none, every POST fails without touching the network, and the days age out of the queue.
- **`count_ui(event: UiCount)`**, granted to `main` alone by its own capability
  (`capabilities/counts.json`): `error_shown` (its code read as `StrictErrorCode`, so `unknown` or
  a code this build does not know is refused with the argument),
  `break_shown`, the webview's `feature_used` values (`versions`, `compare`, `open_in_word`,
  `continue_other_agent`, `prepared_answer_used`, `sample_matter`; never the host's `undo` and
  `restore` or stage E's tools'), and `onboarding_step`. Each enum refuses an unknown field or
  value. They mirror the schema's enums name for name, so the TypeScript bindings can name them.
- **The bridge** admits `FirstAct` from `main` only and `SetPrivacySettings` from the settings
  window only.
- **The lock.** `Counts` makes its file changes with its state's lock released: the state queues
  them, and a second, disk lock orders them. `bump` takes the state's lock alone and never waits
  on the disk, and an off waits for a write under way before it deletes the folder.
- **No document text in a payload** is shown end to end by `crates/desktop/tests/canary.rs`. Its
  session runs in a child process whose own environment holds the canary (`HOME`, `USER`,
  `CLAUSEO_DATA_DIR`, a variable), and on a machine the schema has no value for it uses a fixed
  envelope, so it never depends on the runner's OS version. Windows reads the profile folder
  from the OS (the known folder), not the environment, so there the host's and the agent's home
  is the planted `USERPROFILE`.
- **`os_major` takes macOS 13 to 40** (was 26; decided 2026-11-04 in review). macOS 27 shipped in
  2026, and macOS is numbered by year since 26, so 40 is the 2039 release: headroom for any build
  sent today, with a schema change well before then. Windows stays 10 or 11. The cap is in the
  shared schema (`OsMajor`, `Platform`, `MACOS_LAST`), so S23's Worker reads it from the same
  types; the snapshots and the per-field cap test move with it. No build has sent schema 1 yet, so
  the schema stays 1.
- **Follow-up:** the envelope is read once at start. A change of `ui.language` or of policy
  reaches the next day report or tally after a restart; calling `Counts::set_about` when either
  changes is a later pull request.

## Status

proposed

## Date

2026-11-02
