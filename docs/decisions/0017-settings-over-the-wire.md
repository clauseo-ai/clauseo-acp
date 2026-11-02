# 0017: Display settings over the wire, `settings.locked`, and app-wide events

## Context

The settings window and the main window both show and change the display settings: theme, text
size, language, lite and motion (research 36, problems A and B). `crates/settings` holds them and
already refuses a `UiPatch` that names a key a firm's policy sets (`Error::Locked`), but no host
call reached it, so neither window could read or change a setting, and the host had no way to
send an event that belongs to no conversation stream. Each window needs every change, its own
included, and must never get one on a thread's or matter's stream. Later rows send more app-wide
events the same way: the matter list and summaries of threads no window has open (H4), notices,
and `limits`' `AccountLimitsChanged` (R16).

`crates/protocol` depends on no crate of ours, so it cannot hold `clauseo-settings`' types.

## Decision

- **Wire types.** `protocol` gains `SettingsView { ui: UiSettings, locked: Vec<String> }`, with
  `UiSettings { theme, font_size, language, lite, motion }`, and `UiPatch`, every field optional
  and unknown fields refused. `theme` is `system | light | dark`, `language` `en | hi`, `lite`
  `automatic | on | off`, `motion` `system | reduce | full`, `font_size` a number. `locked` is
  every dotted key policy locks, sorted, `matters.default_location` and `agents.<id>` included,
  since the window shows each locked control as managed and never infers a lock from a value.
  They mirror `clauseo-settings`' `ui` section, which keeps the rules: the font size range, the
  locks, the write. `ui.density` stays in the settings crate and is left off the wire; the app no
  longer uses it. The host converts with exhaustive matches, and a test in `crates/host`
  (`settings::the_wire_mirrors_the_settings_crate_key_for_key_and_value_for_value`) fails when a
  key or a value is in one and not the other.
- **The settings crate.** `ui.font_size` runs from 14 to 20, default 16 (was 12 to 20, default
  14). `ui.language` becomes the closed set `en | hi` (was any BCP 47 tag): the set the app has
  words for, and the only way the wire's closed `language` can mirror it value for value. New
  keys `ui.lite` and `ui.motion`, each with its `UiPatch` field and policy lock. A stored value now
  outside these (a size of 12 or 13, a language such as `fr` or `en-GB`) takes the existing
  invalid-value path: a warning, and the key's default.
- **Query.** `HostQuery::GetSettings` (`"get_settings"`) answers
  `QueryResult::Settings(SettingsView)`, from host memory.
- **Command.** `HostCommand::SetUiSettings { patch: UiPatch }` answers `Done`. It runs as every
  command does: shape (`validate`, which leaves the patch alone), the principal, the receipt, then
  the work. The work turns the patch into the settings crate's (a font size out of range is
  `command.invalid` with the settings crate's message), then calls `Effective::apply_ui_patch`
  on the actor. The actor owns `Effective` alone, so it is the one lock the write needs; commands
  run one at a time and no reader sees half a patch. The commit holds only the receipt
  (`stream: None`): the settings live in `settings.json`, not the log, and the receipt makes a
  repeated `command_id` answer `Done` without writing again.
- **Error code.** `settings.locked`, not retryable: the patch names a key the firm's policy sets.
  The patch is refused whole, even when it also sets unlocked keys or asks for the locked value,
  and nothing is written or sent. Every other `clauseo_settings::Error` stays `internal`.
- **Event.** `HostEvent::SettingsChanged(SettingsView)` (`settings_changed`), the whole view after
  the change, goes to every app subscriber after each successful write, the sender's window
  included, and to no stream subscriber. An empty patch writes nothing and sends nothing. It is
  sent when the write succeeds, before the receipt's commit returns: the settings have changed
  whatever the receipt does, and if that commit fails the caller gets the store's error while the
  windows already show the new value; a retry writes the same values again.
- **`HostHandle::subscribe_app(principal)`** returns an `AppSubscription`: the app-wide events of
  no stream, as `EncodedEvent`s serialised once and shared by every app subscriber. Its rules
  follow `subscribe`: the principal is checked (`transport.unauthorized` for one this version does
  not know); each subscriber has its own `bounded(1024)` channel filled with `try_send`, so a slow
  window never holds up the host or another window; a full subscriber gets nothing until it has
  drained, then gets the current state at once (one `SettingsChanged`), as a view subscriber gets
  a `Reset`; dropping it unsubscribes; every app stream ends when the host stops. It starts empty:
  a window subscribes, then calls `GetSettings`, and since the host answers calls in order no
  change falls between the two. So there is no revision number (research 36 proposed one): one
  actor orders every change, and each window's stream delivers them in that order.
- **Later events** go through the same `publish` in `actor/app.rs`, and each adds its part of the
  current state to the catch-up a lagged subscriber gets: H4's matter list and thread summaries,
  notices, and R16's `AccountLimitsChanged`. None is built here.

## Consequences

- The desktop bridge forwards one app stream to every window. Every window applies each
  `SettingsChanged` it receives, and none applies its own change before the event comes back.
- The webview can change only the `ui` keys, through a patch that refuses every other field, and
  never a locked one. It cannot reach agents, folders or policy.
- The write runs on the actor: one small `settings.json` write with an fsync, on a lawyer's click.
  A write slow enough to matter would move to a thread of its own with `Effective` behind it.
- A firm profile that set `ui.language` to another tag, or `ui.font_size` to 12 or 13, now warns
  and falls back to the default.
- Adding a display setting means a key in `clauseo-settings` (its checklist in `leaf.rs`), the
  field in both wire types and a match arm in `crates/host/src/actor/settings.rs`; the mirror test
  fails until all are done.

## Status

proposed

## Date

2026-11-02
