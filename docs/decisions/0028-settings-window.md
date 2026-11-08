# 0028: The settings window

## Context

The design opens settings in a window of its own on both computers, never as a sheet over the room
(`design/03-system/src/pages/settings/settings-window.tsx`): a matter stays in view beside it, and
on the Mac it opens from Clauseo > Settings… and Cmd+, as every Mac app's settings do. Its frame is
the room's: on the Mac the corner lights in the window's top left (20 px in, centred 26 px down,
where the room's lead puts them), on Windows a bar ending in all three caption buttons, flush top
right, so it can be maximised and snapped beside the room.

Until now `main` was the only window. The bridge already keeps one stream per window label
([0012](0012-desktop-bridge.md)), but nothing removed a window's stream: `main` only goes away
when the app exits. [0015](0015-window-frame.md) asked a second window's record to say why the
page's label-taking window commands reaching it are harmless, or to prevent it.

## Decision

**Window.** A second window, label `settings` (`crates/desktop/src/settings_window.rs`), showing
`ui/app`'s `settings.html`. It is built in code like `main`: 960 by 700 logical px fitted to the
primary monitor's work area and centred (the e2e build opens it unfitted), the https scheme (the
same origin as `main`, so the same local storage), the room's frame (`window_frame::with_frame`,
and on Windows the native window over the maximise button for Snap Layouts, made on the main
thread), the page's CSP on its scripts (`csp::on_scripts`), and in the e2e build `main`'s webview
arguments and init script, since `WebView2` webviews that share a profile must be created alike.
Its title is "Settings" until the page sets it in the app's language. window-state never restores it: it opens at its size, centred, each time.

**Lifecycle.** Made on open, destroyed on close, never hidden. Opening it when it is open
unminimises, shows and focuses it. When a window is destroyed, the bridge drops its stream
(`Bridge::close_window`): its channel, every pump and the app pump (so their host subscriptions),
and its watchdog. Each build is numbered (`Bridge::window_built`, from the counter `open_stream`
numbers from), and the window's own `Destroyed` handler passes that number, so a close that lands
after the window was built again removes only a stream opened before that build, never the new
page's. Only a build that worked is recorded, so a second open that fails on the taken label
(a double click, or the menu and Cmd+, together) does not make the first window's close look
late. The close also leaves a tombstone, the number below which every `open_stream` of that label
belongs to a destroyed window: a call still waiting for the host's start when its window closed
drops its stream when it wakes. With Wry the late-close guard never fires (Tauri removes a destroyed window before its handlers run,
and builds a window on the event loop only after them); it holds whatever order a runtime keeps.
Destroying `main` destroys `settings`, so closing `main` still ends the app.

**Capability.** `capabilities/settings.json` grants `settings` six commands: `dispatch`, `query`,
`open_stream`, `subscribe_app`, `ack` (app events count toward the flow-control window) and
`display_info`, the drag script's two window commands, and `core:window:allow-set-title`, so the
page names its window in the app's language at open and on each language change (Windows shows
the title in the taskbar and Alt+Tab; `main` is not granted it); `capabilities/frame.json` names it
beside `main`, so on Windows and Linux it gets the caption buttons' four. Not granted: `subscribe`,
`unsubscribe`, `answer_permission`, `pick_folder`, `open_external`, `open_file`, `reveal`,
`open_settings`, and the e2e and marks commands. `tauri.conf.json` names `main` and `settings`.

**What the bridge admits.** From `settings`, `dispatch` takes only `SetUiSettings`,
`SetToolSettings` (0030, "Tool switches over the wire"), `SetPrivacySettings` (0036),
`SetSkillPack` (0040; refused from `main`), `RefreshLimits`, `SetUpdateMode` (0037) and
`CheckForUpdate` (0037), the connector commands but `DismissCaseLawHint` (R24: Check, Add,
AddApiKey, AddCredits, SignOut, Remove, RefreshBalance; `main` sends only `DismissCaseLawHint`
and `RefreshBalance`, so the window that shows agent output never adds a key or spends credits),
and `query` only `GetSettings`, `GetUpdate` (0037) and
`ListSkillPacks` (0040; `main` reads it too, for the matter picker) (`Bridge::admit`, `Bridge::query`, keyed on the calling
webview's label); everything else is `command.invalid` before the host sees it. A window of any other label is refused every
command and read. Of the app feed (`subscribe_app`), whose first frames carry the whole matter
and thread lists since 0023, the settings window gets `settings_changed` and, for its Connectors section (R24),
`connectors_changed`, and any other window none of it: the
bridge drops every other app event before it is sent or counted toward acks (`AppEvents` in
`bridge/pump/feeds.rs`, an allow-list keyed on the label, so an app event the host adds later
stays out). A settings section that needs more (plans, matters) widens these lists in its own
change.

**Command.** `open_settings`, no arguments, `Result<(), HostError>` (`internal` when the window
cannot be built): opens or focuses the window. Granted to `main` only; the page calls
`commands.openSettings()`.

**Menu.** On the Mac only, the app menu is Tauri's default with "Settings…" on Cmd+, after About
and its separator, then a separator; choosing it opens or focuses the window. Windows and Linux
get no menu: it would be a bar on the undecorated window.

## Consequences

- Script in either page can aim the drag and caption commands at the other window (Tauri's
  window commands take a `label`). That is harmless here: neither window holds state the bridge
  or the host does not keep; dragging, zooming, minimising or maximising the other is visible and
  undone by hand; closing `settings` only destroys it; and closing `main` was already in the
  page's reach and ends the app as its close button does. No granted command sets a window's
  size, position or content, or reaches a plugin. A third window needs its own record (a test
  counts the builders).
- `set_title` takes a `label` too, so the settings page could retitle `main`. Tauri's capabilities
  have no scope for core window commands, and a label-free command of our own would be a new
  command for a string, so it is not scoped. It is harmless: the title is text in the taskbar,
  Alt+Tab and the Mac's Window menu; it carries nothing to the host, opens nothing, and the
  page could already close `main` outright. `main`'s page cannot set any title.
- XSS in the settings page reaches the settings and nothing else of the host: no matter, thread,
  folder, file, link or permission answer. That holds for the app feed because the bridge filters
  the settings window's to `settings_changed` and `connectors_changed` (R24's connector state,
  which holds no token or key); U12 and U15 widen it when their sections read more. XSS there
  can add a connector, paste a key or open a pay link's recharge (each through the host's own
  checks, the pay link only through the native confirm); the main window cannot.
- Main-only behaviour stays keyed on `MAIN_WINDOW`: the scoreboard's capabilities, the limits
  subscription `open_stream` starts (`Bridge::open_stream` follows the limits for `main` alone)
  and the focus refresh (`on_window_event` in `lib.rs` dispatches `Shown` for `main` alone).
  The settings window's channel carries no limits, and its focus reads none (the amendment to
  [0027](0027-limits-in-host.md): U12 opens the limits for Settings when Plan and usage reads
  them). Closing a window drops its limits pump with the rest of its feeds.
- Settings' "Try again" (`RefreshLimits { why: TryAgain }`) reaches the limits model from either
  window: the model ignores a TryAgain within 10 s of the last read's start, whatever window sent
  it, and re-emits the view; the bridge's throttle on this window is defence in depth.
- The mock runtime never removes a destroyed window, so the reopen after a close is tested at the
  bridge (`bridge/tests/app.rs`), not through Tauri. The window itself, its frame and the menu are
  checked by hand on each OS.

## Status

proposed

Amended 2026-11-03: U12 opened the limits for the settings window's Plan and usage. `open_stream`
follows them on its channel as on `main`'s (`follows_limits` in `lib.rs`), `AppEvents` passes
limits events for it while its app feed stays `settings_changed` alone, `dispatch` admits
`RefreshLimits` from it beside `SetUiSettings` (`Shown` as the section opens, `TryAgain` from
its button), and its focus dispatches `Shown` as `main`'s does. Any other window: nothing until
its own ADR line opens it. It follows no limits, sends nothing, and its channel carries no app or
limits event (`AppEvents::Nothing` in `crates/desktop/src/bridge/pump/feeds.rs`), where before
it got `settings_changed`: fail-closed, as this record's allow-lists are.

The risk: `TryAgain` reads now whatever the last read's age, so Try again pressed over and over
in Settings would have the host start a read for each press. The bridge throttles the settings
window's `RefreshLimits` to one per agent per 10 s (`agent: None`, every agent, is a key of its
own; `crates/desktop/src/bridge/throttle.rs`). One it holds back is dropped quietly: `dispatch`
answers `Done` and sends the host nothing. No error code means "too soon", and the page's next
limits view says what came of the last read anyway. `main`'s refreshes pass as before. Host issue
#220 will have the host keep 10 s per agent kind between `TryAgain`s from any window, sending the
current limits again so a spinner still settles; the bridge's throttle stays as defence in depth.

## Date

2026-11-02
