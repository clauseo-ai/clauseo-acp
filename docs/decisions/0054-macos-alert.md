# 0054: macOS's widening question as an NSAlert

## Context

R18 asks the lawyer, in a native dialog outside the webview, whether to widen a matter's reach:
allow once, allow for the matter, refuse, or refuse for the matter (`NativeDialogs`, the host's
seam, `crates/host/src/dialogs.rs`). On Windows the task dialog's verification checkbox carries
"Remember for this matter". rfd, which draws the app's other confirms, has no checkbox and at most
three buttons, so on macOS it can't offer "Don't allow for this matter". `NSAlert` can: its
suppression button is a checkbox under the buttons.

`NSAlert` is AppKit. Desktop's only Apple binding so far is `objc2-core-foundation`. The AppKit
binding is `objc2-app-kit`. Its methods take `objc2-foundation`'s `NSString`, and an alert is
built with `objc2`'s `MainThreadMarker`. All three are already in `Cargo.lock` through rfd
(objc2 0.6.5, objc2-foundation 0.3.2, objc2-app-kit 0.3.2), which uses them for its own
`NSAlert` and panels.

## Decision

- Desktop depends on `objc2`, `objc2-foundation` and `objc2-app-kit`, on macOS only
  (`[target.'cfg(target_os = "macos")'.dependencies]`). They're pinned at the workspace to the
  lock's versions, with default features off and the crate line naming each feature it uses:
  - `objc2`: `std`;
  - `objc2-foundation`: `std`, `NSString`;
  - `objc2-app-kit`: `std`, `NSAlert`, `NSApplication`, `NSButton`, `NSCell`, `NSControl`,
    `NSResponder`, `NSView`.

  This adds no new crate to the lock. Only `NSCell` is a feature rfd doesn't already turn on, and
  its one extra dependency, `bitflags`, is already a dependency of objc2-app-kit in the lock.
- `crates/desktop/src/dialogs/ask/macos.rs` draws the question:
  - Don't allow is added first, and Allow second. Don't allow's key equivalent is Escape, which
    replaces the Return that `NSAlert` gives its first button, so Return presses nothing and
    never allows.
  - The suppression checkbox is titled "Remember for this matter".
  - `runModal` runs on the main thread (`AppHandle::run_on_main_thread`), while the caller,
    off the host's actor, waits on a channel.
  - The code uses only the typed, safe API, with no `unsafe` and no `msg_send!`.
- The answer is the same pure mapping as Windows' (`ask/answer.rs`):
  - Allow with the box checked is for the matter; Allow alone is once;
  - Don't allow with the box checked refuses for the matter; Don't allow alone refuses;
  - a session ended any other way refuses, checked or not.

  `runModal` answers which button ended the alert, not how, so Escape counts as a click on
  Don't allow: with the box ticked it refuses for the matter. Windows refuses on Escape whatever
  the box says. Neither ever allows.
- ARCHITECTURE.md's desktop row lists the three crates as macOS only.

## Consequences

- macOS offers all four answers, the same as Windows. Linux keeps rfd's three buttons, without
  "Don't allow for this matter".
- The AppKit code can't be compiled on the Linux machines where most work happens: objc2's build
  script needs Apple's C compiler. Only CI's `aarch64-apple-darwin` job on `macos-15` builds,
  lints and tests it, and that happens in the merge queue (pull requests skip it). Keep the file
  small, and keep its logic in the pure mapping, which is tested on every OS.
- A bump of rfd that moves objc2's versions moves these three with it. The workspace pins have
  to follow the lock, so the build doesn't end up with two copies.
- Another AppKit use names its features on the same crate line, and its record names its own.

## Status

accepted (code owner, 2026-11-08)

## Date

2026-11-08
