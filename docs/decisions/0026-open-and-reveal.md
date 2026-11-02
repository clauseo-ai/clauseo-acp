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

- **`open_external { url }`.** At most 2,048 bytes, parsed by the `url` crate. Only `https` and
  `http` with a host and no user name or password, and `mailto` with plain addresses (printable
  ASCII, no spaces, at most 254 bytes, an `@`). Anything else is `command.invalid`, before any
  dialog. Then a native confirm drawn by Rust names where the link goes: the host in its ASCII form
  (an international name as `xn--`, so a look-alike cannot pass), with the port when the link names
  one, or the mail addresses. Its words follow the app's language (`ui.language`, read from the
  host when the dialog opens), in English and Hindi (`open/words.rs`, copy H11-01 to H11-07).
  Declined, nothing opens and the call fails with `transport.unauthorized`, which the window
  does not show. Accepted, the OS opens the URL as the `url` crate wrote it back, in the default
  browser or mail app. The webview never navigates.
- **`open_file { file: FileRefId }`.** The webview names a file only by a ref the host issued
  (`RegisterFile`, and H8's papers). The host answers what the ref names and the folder of the
  matter that owns it, refusing a ref it did not issue and a file outside that matter's folder at
  call time. The desktop checks again, then opens the file with its default app, by its resolved
  path: the file and the folder resolve (symlinks and `..` followed), the file lies in the folder,
  it is a regular file (so never a folder or an app bundle), and its type is one of
  `DOCUMENT_TYPES`: documents, sheets, slides and images. Not on the list: programs, scripts,
  shortcuts, web pages, SVG and Office files with macros (`docm`, `xlsm`, `pptm`, `dotm`).
  Refusals are `command.invalid`, or `matter.folder_missing` when the folder is gone; a failure
  of the OS to open is `internal`. No confirm: the lawyer clicked one of their own documents.
- **`reveal { target }`.** `target` is `{ "matter": MatterId }` or `"app_data"`, never a path.
  The host answers the matter's folder (`command.unknown_matter` otherwise); the app data folder
  is the one `Locations::resolve` found at start-up. The folder is shown selected in its parent
  folder (the opener plugin's `reveal_item_in_dir`), which never launches what it shows, even a
  folder that is a bundle. A matter's folder that is not there is `matter.folder_missing`.
- The host gains two Rust-only `HostHandle` methods for the bridge, `locate_file` and
  `matter_folder`, authorised for the desktop's own principal only. Nothing changes on the wire
  between host and desktop, in the protocol or in the store; no error code is added.
- The confirm, the opener and the host sit behind traits (`LinkConfirm`, `Opener`, `Locate`), so
  tests run every refusal without a window and never open anything.

## Consequences

- The page reaches thirteen typed commands. A script in the page can now ask the lawyer to open a
  link (they see where it goes and must accept), open one of the matter's documents in its app,
  and show a matter's folder or the app data folder. It cannot open a path it names, a program, or
  any link without the confirm.
- A document's own app still runs whatever the document holds (an old `.doc` with macros, a PDF
  reader's scripts); that is the app's control, as it is when the lawyer double-clicks the file.
- Between the desktop's check and the OS opening the file, a process that can write the matter
  folder can swap a folder on the path for a link. The agent can, until stage C's sandbox; it
  could then open a document of a listed type outside the matter, not a program.
- A new type in `DOCUMENT_TYPES`, a new scheme, a confirm that is skipped, or a new reveal target
  needs a new record.

## Status

proposed

## Date

2026-11-02
