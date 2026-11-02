# 0019: The app stream to the window, and allow-lists for `dispatch` and `query`

## Context

[0017](0017-settings-over-the-wire.md) gave the host display settings over the wire
(`GetSettings`, `SetUiSettings`) and app-wide events (`HostHandle::subscribe_app`, today only
`SettingsChanged`). The room needs them in the real app: it reads and changes the settings and
follows every change, whoever made it. Later host rows add more reads and commands the same way:
H8's `list_papers` (#131), H3's `relocate_matter` (#135), H4's matter list and thread summaries on
the app stream.

The reads and commands already reach the host. [0012](0012-desktop-bridge.md)'s `query` and
`dispatch` take protocol's own `HostQuery` and `CommandEnvelope`, which decode and validate in
Rust, and run as `LocalDesktop`. Only the app stream had no path to the page. 0012 asks for a new
record for a tenth command.

There were two ways to carry the new calls. One typed command per call means four commands now,
two more for #131 and #135, and one more per host row after that, each with its own permission,
list lines and binding. Each would repeat a variant of an enum that Rust already checks, and the
security would be no better. The other way is the checked passthrough that exists. Its weakness
was `dispatch`'s deny-list: it refused three commands and let any other through, so a command the
host added later reached the page without anyone deciding it should.

## Decision

- **Reads and commands stay on `query` and `dispatch`.** Their request types stay the host's own
  enums, decoded and validated in Rust.
- **Both are allow-lists.** `Bridge::admit` matches every `HostCommand` variant by name, with no
  catch-all, and `query` matches every `HostQuery` variant the same way. A variant the host adds
  does not compile in `crates/desktop` until the bridge rules on it.
  - `admit` refuses `AnswerPermission`, `SetMode`, `SetConfigOption`, and a `CreateMatter` whose
    folder ref this run's `pick_folder` did not issue, as before, all with `command.invalid`.
  - Slots wait as comments for `ListPapers` (#131), `RelocateMatter` (#135), H5 and H20's
    `UpdateMatter`, `OpenMatter` and `ForgetMatter`, and H20's `GetDefaultLocation`.
    `RelocateMatter` will take only a folder ref this run's `pick_folder` issued, as
    `CreateMatter` does. H20's `CreateMatter` source `default_location` carries no folder ref and
    passes; the folder-ref check stays on `new_folder` and `existing_folder`.
- **One new command, `subscribe_app()`.** It takes no arguments, and it needs `open_stream` first
  (otherwise it fails with `command.invalid`).
  - It subscribes the window's existing channel to `subscribe_app(LocalDesktop)`. Each message is
    the host's encoded `HostEvent` (`{"settings_changed": SettingsView}`), framed and counted like
    a stream's (0012's 8 KB messages, the 256 KB cap, the watchdog). It carries no position, so
    an `ack` names none for it.
  - It starts with nothing. The page subscribes, then reads `GetSettings`. The host answers in
    order, so no change falls between the two (0017).
  - Calling it again restarts it. A new `open_stream` (a reload) drops it with the window's other
    subscriptions.
  - A pause or a watchdog fire drops it with the streams. The restart that follows subscribes
    again, reads `GetSettings`, and sends one `settings_changed` before any event of the new
    subscription. So the window may miss steps but never ends on a stale view.
  - The host refuses `subscribe_app` only once it has stopped, so a failed restart is not
    retried: the window stops following the app stream.
- **The capability.** `allow-subscribe-app` is granted by `capabilities/main.json` only. It is in
  `build.rs`'s `RELEASE` and `commands.rs`'s `COMMANDS`, and the test that compares the lists
  (`every_list_of_commands_agrees`) and the bindings drift test cover it.
- **The room.** In the real app, `nextHostOf` (`ui/app/src/room/host-next.ts`) gives the room the
  settings part of `NextHost`, built on `query`, `dispatch` and the app stream. Its other parts
  stay empty until the host has their rows. Tests, the probe and the copy review keep the stand-in.

## Consequences

- The page reaches ten typed commands. The threat model gains two things:
  - Every host command and read the page can send is named in Rust. A new one fails closed until
    a change to this crate admits it.
  - The new command is read-only: it takes nothing from the page and only adds messages to the
    window's own channel, under the same byte cap.
- A host row that adds a read or a command needs one arm in `admit` or `query` and a test. It
  needs no new permission and no new command, unless its arguments need a check that decoding
  cannot do (a folder ref, a native confirm). In that case the check goes in `admit`.
- Every app event reaches every window that subscribed. A window drops what it does not need
  (0017).
- The room is unreleased, so the display values it kept in localStorage before this change are
  not carried over. With a host, localStorage becomes a cache of the host's last answer, read
  only for the next start's first frame; the host's answer then replaces it. A start shows the
  wrong settings for a moment only when they changed while the room was not running.
- R12's internal window sends the same calls as before. A bridge test,
  `every_command_and_read_the_windows_send_today_is_admitted`, lists them and fails if one is
  refused.

## Status

proposed

## Date

2026-11-02
