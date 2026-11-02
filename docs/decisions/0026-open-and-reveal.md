# 0026: Opening links and documents, and showing folders

## Context

Answers render links and papers, and the screens link to pages outside the app: the plans'
pricing, "Read the code", the WebView2 download. A matter's folder and the app data folder must
be shown in Finder or File Explorer ("Show in Finder", "Add papers", "Where versions are kept"),
and a paper must open in its own app (Word for `.docx`). [0001](0001-rust-webview-boundary.md)
names `open_external` and `open_file` and says how they are bounded; nothing built them, so every
click did nothing. Script in the page is the attacker of [the threat model](../threat-model.md):
whatever these commands open, a prompt injection in a document can ask the page to open too. An
agent can also write any file into the matter folder, so "a file in the matter" can be a program,
a script, a shortcut, a web page or a macOS app bundle.

## Decision

Three typed async commands in `crates/desktop/src/open.rs`, each with its own `allow-<command>`
permission, granted by `capabilities/main.json` only, listed in `build.rs`'s `RELEASE` and
`commands.rs`'s `COMMANDS`, and in the TypeScript bindings. No plugin and no plugin permission is
added: the dialog and opener plugins are called from Rust, as before.

- **`open_external { url }`.** At most 2,048 bytes as given and as the `url` crate writes it back
  (`open/link.rs`). Only:
  - `https` and `http` with no user name or password, to a host name of at most 253 bytes and
    labels of at most 63, never `localhost` or a name under it, nor a loopback, private,
    link-local, unspecified or broadcast address (IPv4, IPv6, or IPv4 written as IPv6);
  - `mailto` to one or more addresses, each `local@domain` with both parts non-empty in RFC
    5322's atext and `.`, at most 254 bytes in all, with at most a `subject` and a `body` (keys
    without case, each once). Any other header (`to`, `cc`, `bcc`, `attach`, `from`, ...) or a
    fragment is refused, since it would add a recipient or an attachment the confirm does not
    name.

  Anything else is `command.invalid`, checked before the settings are read and before any dialog.
  Then a native confirm drawn by Rust names where the link goes: the host in its ASCII form (an
  international name as `xn--`, so a look-alike cannot pass), with the port when the link names
  one, or the mail addresses. Its words follow the app's language (`ui.language`, read from the
  host when the dialog opens), in English and Hindi (`open/words.rs`, copy H11-01 to H11-07).
  Cancel is the first button, so it is the default and focused one (Return and Enter, and Space
  on Windows) and a stray key never accepts; only a press of the open button accepts, and Escape,
  closing the dialog or the app closing declines (`dialogs::confirm`, which U4's "always allow"
  confirm uses too). The dialog sits over the main window. One confirm at a time: a second call
  while one is open, or within a second of a decline, is `command.conflict` before anything is
  drawn. Declined, nothing opens and the call fails with `transport.unauthorized`, which the
  window does not show. Accepted, the OS opens the URL as the `url` crate wrote it back, in the
  default browser or mail app. The webview never navigates.
- **`open_file { file: FileRefId }`.** The webview names a file only by a ref the host issued
  (`RegisterFile`, and H8's papers). The host answers what the ref names and the folder of the
  matter that owns it, refusing a ref it did not issue and a file outside that matter's folder at
  call time. The desktop checks again, then opens the file with its default app, by its resolved
  path: the file and the folder resolve (symlinks and `..` followed), the file lies in the folder,
  it is a regular file (so never a folder or an app bundle), it has no execute bit (macOS and
  Linux), and its type is one of `DOCUMENT_TYPES`: documents, sheets, slides and images. Not on
  the list: programs, scripts, shortcuts, web pages, SVG and Office files with macros by name
  (`docm`, `xlsm`, `pptm`, `dotm`). On macOS the file opens in the default app for the type
  that was checked, named (`open -a <app> -- <file>`; the app from
  `LSCopyDefaultApplicationURLForContentType` for the extension's type, `open/mac_app.rs`), so a file swapped for a link after the checks,
  or one with its own "Open With" setting, opens as a document in that app and never as a program.
  On Windows the shell picks the app by the extension of the name, which was checked. One open at
  a time and at most one a second (`command.conflict`). Refusals are `command.invalid`, or
  `matter.folder_missing` when the folder is gone; a failure of the OS to open, or a type macOS
  has no app for, is `internal`. No confirm: the lawyer clicked one of their own documents.
- **`reveal { target }`.** `target` is `{ "matter": MatterId }` or `"app_data"`, never a path.
  The host answers the matter's folder (`command.unknown_matter` otherwise); the app data folder
  is the one `Locations::resolve` found at start-up. The folder is shown selected in its parent
  folder (the opener plugin's `reveal_item_in_dir`), which never launches what it shows, even a
  folder that is a bundle. A matter's folder that is not there is `matter.folder_missing`.
- **`open_url_unconfirmed`**, for Rust's own callers only (stage D's R20 opens the captured
  sign-in page with it): `open_external`'s link check narrowed to https, then the default browser,
  with no confirm. It is a crate-internal function, not a command: no capability, handler or
  binding names it, and a test checks so. `open_external` shares its check and the opener, and
  adds the confirm.
- The host gains two Rust-only `HostHandle` methods for the bridge, `locate_file` and
  `matter_folder`, authorised for the local desktop's principal only. Every bridge call runs as
  that principal, so the principal check does not set the webview apart: what bounds the page is
  that it can name only refs the host issued (for files H8 listed, or the lawyer picked), and the
  host's containment check on each. Nothing changes on the wire between host and desktop, in the
  protocol or in the store; no error code is added.
- The confirm, the opener and the host sit behind traits (`LinkConfirm`, `Opener`, `Locate`), so
  tests run every refusal without a window and never open anything.

## Consequences

- The page reaches thirteen typed commands. A script in the page can now ask the lawyer to open a
  link (they see where it goes and must accept), and show a matter's folder or the app data
  folder. It can also open any listed document of any matter in its app without a click, since
  Rust cannot tell a click from a script; the gate holds that to one a second. It cannot open a
  path it names, or any link without the confirm.
- A document's own app still runs what its own policy allows (an old `.doc` or `.xls` with
  macros, an `.rtf` with embedded objects, a PDF reader's scripts), as when the lawyer
  double-clicks the file.
- Between the desktop's check and the OS opening the file, a process that can write the matter
  folder (the agent, until stage C's sandbox) can swap the file, or a folder on its path, for a
  link. On macOS the named app then opens whatever the link points at as a document of the checked
  type: a document outside the matter, not a program (an app bundle given to a document app as a
  file does not launch). On Windows a junction on the path needs no privilege, a symlink for the
  file itself does; either way the shell opens by the checked extension, so the gap is the same: a
  document of a listed type outside the matter. A Windows lookup of the type's app
  (`AssocQueryString`) would not narrow it further, and is not built.
- A file with an execute bit does not open, even a real document; files on volumes that mark
  every file executable (FAT, exFAT, some network shares) cannot be opened from Clauseo on macOS.
- On macOS, Escape cancels a confirm whose cancel button macOS recognises by its English title
  ("Cancel"); in Hindi Escape may do nothing (the dialog stays, nothing opens), since the dialog
  plugin cannot give the button its key.
- A new type in `DOCUMENT_TYPES`, a new scheme, a confirm that is skipped, or a new reveal target
  needs a new record.

## Status

proposed

## Date

2026-11-02
