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

- **Files.** `<data_dir>/first_act_done` (written at the first act, never deleted) and
  `<data_dir>/counts/`: `install_id`, `today.json`, `queue/<day>.json`, `tally/<week>.json`. Each
  is written whole through a temp file and a rename, only in `src/write.rs`. A file that does not
  read as its type is removed.
- **Modes.** Off (the switch or policy) counts nothing, deletes `<data_dir>/counts/`, and cancels
  a POST in flight, which is never retried. On before the first act counts in memory only. On
  after it saves and sends. Turning counts on again makes a new install id.
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
- The host and desktop wiring (the settings keys, `FirstAct`, the call sites, the `reqwest`
  upload) comes in S21's next pull request; until then nothing calls this crate.

## Status

proposed

## Date

2026-11-02
