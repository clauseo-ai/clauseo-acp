# 0015: The window frame: the page's toolbar as the title bar

## Context

The room's design makes its 64 px toolbar the window's title bar on both platforms: on the Mac
the native traffic lights stand in the toolbar where its corner lights are drawn, and on Windows
the toolbar ends in Windows 11's minimise, maximise or restore, and close, and hovering maximise
shows Snap Layouts. The toolbar's empty space drags the window, and a double-click maximises on
Windows and zooms on the Mac. The page has to move, size and close its own window for that, and
[0001](0001-rust-webview-boundary.md) asks for a record for each capability change.

R12's internal window stays the default page until U5 and keeps its OS frame, so the frame is
chosen by the same build switch as the page, `VITE_CLAUSEO_WINDOW=room`.

Snap Layouts appears only when the window under the pointer answers `WM_NCHITTEST` with
`HTMAXBUTTON`. WebView2 covers the client area with its own child window, which answers
`HTCLIENT`, so subclassing the top-level window sees nothing. WebView2's non-client region API
names minimise, maximise and close regions (`COREWEBVIEW2_NON_CLIENT_REGION_KIND_MAXIMIZE`), but
only through `ICoreWebView2CompositionController4`, which needs visual hosting; wry 0.57 creates a
windowed controller (`CreateCoreWebView2Controller`, `wry/src/webview2/mod.rs`), where the
`IsNonClientRegionSupportEnabled` wry turns on gives only `app-region: drag`.

## Decision

- **The switch.** `crates/desktop/src/window_frame.rs` reads `VITE_CLAUSEO_WINDOW` at compile time
  (`option_env!`; `build.rs` reruns when it changes). `room` gives the page's frame; anything else,
  or unset, keeps the OS frame. The window is built with its frame, so nothing flashes at start.
  `tauri-plugin-window-state` no longer saves or restores decorations, so a frame saved by one
  build never reaches the other. U5 removes the switch on both sides.
- **The Mac.** An overlay title bar with its title hidden, and the native traffic lights moved
  with `traffic_light_position` onto the toolbar's first corner light. `CornerLights native`
  keeps the corner lights' space and paints nothing under them.
- **Windows and Linux.** No OS decorations. Tauri keeps an undecorated window's system shadow and
  resize borders. On Windows a small transparent native child window covers the page's maximise
  button and answers `WM_NCHITTEST` with `HTMAXBUTTON`, so Windows shows Snap Layouts; a press on
  it maximises or restores the window in Rust, and while the pointer is over it Rust sets
  `data-pointer` on the button so the page draws its hover and press. Tauri itself covers an
  undecorated window's borders the same way (`TAURI_DRAG_RESIZE_BORDERS`,
  `tauri-runtime-wry/src/undecorated_resizing.rs`). The geometry is a pure function of the
  window's width and scale, checked against the toolbar's tokens.
- **The capability.** `capabilities/main.json` grants `main` six core window commands, one
  permission each, and no permission set: `start_dragging` and `internal_toggle_maximize` (Tauri's
  drag script, for `data-tauri-drag-region`), `minimize`, `toggle_maximize`, `close` and
  `is_maximized` (the caption buttons, and the maximised state that names the middle one). The
  list is `WINDOW_PERMISSIONS` in `window_frame.rs`; a test checks the file grants these and no
  other core permission. They are granted in every build; the default build's page never calls
  them. No event permission: the page reads the maximised state again after its own `resize`
  event, not from Tauri's window events.
- **Where the page calls them.** `@tauri-apps/api/window` is imported only in
  `ui/app/src/transport/window-controls.ts`; the hook in `ui/app/src/window/` takes its
  `WindowControls`, and `caption.tsx` stays a part that takes handlers.

## Consequences

- Script in the page can move, maximise, minimise and close the window, and ask whether it is
  maximised. That is all it gains: no command sets a window's size, position, title, decorations
  or any other property, and none reaches another plugin.
- Tauri's window commands take an optional `label`, so script in the page can aim them at any
  window of the app. `main` is the only window today. A second window (H14's settings window)
  must not be reachable that way: it either gets these commands through label-free commands of
  our own, or its record shows why reaching it is harmless.
- A closed window goes through the same exit path as the OS close button.
- The frame's native code is Windows-only and lives in `crates/desktop/src/window_frame/`.
- The double-click on the Mac zooms as Tauri's drag script does; it does not read the system's
  "Double-click a window's title bar to" preference.

## Status

proposed

## Date

2026-11-02
