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
  (`option_env!`, which rustc tracks, so the crate rebuilds when it changes). `room` gives the
  page's frame; anything else, or unset, keeps the OS frame. The window is built with its frame,
  so nothing flashes at start. `tauri-plugin-window-state` no longer saves or restores
  decorations, so a frame saved by one build never reaches the other. U5 removes the switch on
  both sides.
- **The Mac.** An overlay title bar with its title hidden, and the native traffic lights moved
  with `traffic_light_position` onto the toolbar's first corner light. `CornerLights native`
  keeps a box as wide as the native lights (`--lights-native-w`), so the gap after them holds,
  and paints nothing under them. The position and the width come from the close button's frame
  as measured on macOS 27.2 (`window_frame/geometry.rs`), not from the running system: Tauri
  2.12.4 has no setter for the position after the window is built, wry applies its stored
  position again on every layout, so buttons moved by us would be put back, and reading the
  frames would take a new AppKit binding dependency. Other macOS versions are a follow-up.
- **Windows and Linux.** No OS decorations. Tauri keeps an undecorated window's system shadow and
  resize borders. On Windows a small transparent native child window covers the page's maximise
  button and answers `WM_NCHITTEST` with `HTMAXBUTTON`, so Windows shows Snap Layouts; a press on
  it maximises or restores the window in Rust, and while the pointer is over it Rust sets
  `data-pointer` on the button so the page draws its hover and press. The native window is named
  with Windows' own words for Maximise and Restore, read from the window's system menu in the
  system's language, and renamed as the window maximises and restores, so a screen reader
  exploring by pointer hears the button's name. Tauri itself covers an undecorated window's
  borders the same way (`TAURI_DRAG_RESIZE_BORDERS`,
  `tauri-runtime-wry/src/undecorated_resizing.rs`). The geometry is a pure function of the
  window's width and scale, checked against the toolbar's tokens and the rules that place the
  page's caption buttons.
- **The capabilities.** One permission per command, no permission set, no event permission.
  - `capabilities/main.json` grants `main` the two commands of Tauri's drag script, in every
    build: `start_dragging` (a press on `data-tauri-drag-region`) and
    `internal_toggle_maximize` (a double-click there). The default build's page has no drag
    region. The list is `DRAG_PERMISSIONS`.
  - `capabilities/frame.json` grants `main` the caption buttons' four: `minimize`,
    `toggle_maximize`, `close` and `is_maximized` (the maximised state names the middle
    button). Its `platforms` are Windows and Linux, since the Mac's own lights need none, and
    `window_frame::grant_caption_commands` adds it at setup only in the room build, as
    `e2e::grant_scoreboard` adds `e2e.json`; `tauri.conf.json` names only `main`. The list is
    `CAPTION_PERMISSIONS`.
  - Tests hold both files to their lists and check, through Tauri's own access check, that the
    caption commands answer only with `frame.json` and never on the Mac. The page reads the
    maximised state again after its own `resize` event, not from Tauri's window events.
- **The page keeps its side.** The native controls stand at fixed places in the window: the Mac's
  lights at the toolbar's first corner light, Windows' maximise window flush with the top right.
  So in the room build (`data-frame="own"` on `<html>`) the toolbar stays at the window's top at
  every size, the caption buttons flush with its top right, the pinned question below it, and
  the lights' box fixed where the native lights stand. The rules are in
  `ui/components/src/clauseo/responsive.css`, and a test fails when one is missing.
- **Where the page calls them.** `@tauri-apps/api/window` is imported only in
  `ui/app/src/transport/window-controls.ts`; the hook in `ui/app/src/window/` takes its
  `WindowControls`, and `caption.tsx` stays a part that takes handlers.

## Consequences

- Script in the page can drag the window, maximise or restore it, and in the room build on
  Windows and Linux minimise and close it and ask whether it is maximised. That is all it gains:
  no command sets a window's size, position, title, decorations or any other property, and none
  reaches another plugin.
- Tauri's window commands take an optional `label`, so script in the page can aim them at any
  window of the app. `main` is the only window, and a test fails when the app builds another. A
  second window (H14's settings window) must not be reachable that way: it either gets these
  commands through label-free commands of our own, or its record shows why reaching it is
  harmless.
- A closed window goes through the same exit path as the OS close button.
- The frame's native code is Windows-only and lives in `crates/desktop/src/window_frame/`.
- The double-click on the Mac zooms, as Tauri's drag script does. It does not follow the system's
  "Double-click a window's title bar to" preference (`AppleActionOnDoubleClick`: zoom, fill,
  minimise or nothing): the drag script calls `internal_toggle_maximize` itself, so following
  it needs a command of our own and its own record. A follow-up.
- Windows' window menu (Restore, Move, Size, Minimise, Maximise, Close) does not open on a
  right-click of the toolbar: the right-click reaches the page, and the native window over the
  maximise button swallows it. Opening it from the page needs a command that runs
  `GetSystemMenu` and `TrackPopupMenu`, a new command with its own record. A follow-up.
- The maximise button's tooltip never shows on Windows, since the native window takes the
  pointer; Snap Layouts shows in its place.
- A screen reader reading under the pointer meets Tauri's resize-border window
  (`TAURI_DRAG_RESIZE_WINDOW`) over the whole page while the window is restored: it is cut down
  to the edges only for mouse hit-testing, which UI Automation ignores. Maximised, it shrinks
  away and our window's name is read. This is Tauri's, for every undecorated window; raising our
  window above it would cost resizing from the top edge over the button. A follow-up, upstream.

## Status

proposed

## Date

2026-11-02
