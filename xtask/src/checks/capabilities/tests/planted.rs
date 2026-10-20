//! The planted cases of the capabilities check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A capability file that grants nothing forbidden passes.
    clean_capability: Passes, inspects 1 => super::cases::clean_capability,
    /// A workspace with no desktop crate stops at the root manifest, and the zero line says so.
    desktop_crate_missing: FailsWith(&[
        "Cargo.toml:1: the desktop crate (package `clauseo-desktop`) is not a workspace member\n    \
         fix: restore crates/desktop or update xtask/src/checks: these checks guard its capabilities, \
         plugins and Tauri config",
    ]), inspects 0 => super::cases::desktop_crate_missing,
    /// A desktop crate with no capabilities folder fails at line 1 and stops the check.
    folder_missing: FailsWith(&[
        "crates/desktop/capabilities:1: the capabilities folder `crates/desktop/capabilities` is missing\n    \
         fix: restore it with main.json: Tauri reads the window's grants from it, and this check guards them",
    ]), inspects 0 => super::cases::folder_missing,
    /// An empty capabilities folder inspects nothing, and the zero line names the normal fix.
    empty_folder: FailsWith(&[
        "0 capability files inspected in the desktop crate's capabilities/\n    \
         fix: capabilities looks for capability files in the desktop crate's capabilities/; restore them, \
         or, if they moved, update the consts xtask/src/checks/capabilities.rs builds that \
         place from",
    ]), inspects 0 => super::cases::empty_folder,
    /// `core:default` as a string value fails at the line of that string.
    core_default_value: FailsWith(&[
        "crates/desktop/capabilities/main.json:4: capability grants `core:default`\n    \
         fix: list only the narrow permissions the window needs; `core:default` also grants event emit and \
         devtools toggling",
    ]), inspects 1 => super::cases::core_default_value,
    /// `core:default` as a key fails at the line of that key.
    core_default_key: FailsWith(&[
        "crates/desktop/capabilities/main.json:3: capability grants `core:default`\n    \
         fix: list only the narrow permissions the window needs; `core:default` also grants event emit and \
         devtools toggling",
    ]), inspects 1 => super::cases::core_default_key,
    /// `remote` as a key fails at the line of that key.
    remote_key: FailsWith(&[
        "crates/desktop/capabilities/main.json:3: capability file mentions `remote`\n    \
         fix: remove the remote block, or reword the text: a remote page must never reach the IPC, and \
         `remote` is matched anywhere",
    ]), inspects 1 => super::cases::remote_key,
    /// `remote` in prose, as a string value, fails and says how to reword it.
    remote_value: FailsWith(&[
        "crates/desktop/capabilities/main.json:3: capability file mentions `remote`\n    \
         fix: remove the remote block, or reword the text: a remote page must never reach the IPC, and \
         `remote` is matched anywhere",
    ]), inspects 1 => super::cases::remote_value,
    /// Each forbidden prefix (`fs:`, `shell:`, `http:`, `opener:`, `dialog:`) fails on its own
    /// line.
    forbidden_prefixes: FailsWith(&[
        "crates/desktop/capabilities/main.json:4: capability grants `fs:allow-read`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
        "crates/desktop/capabilities/main.json:5: capability grants `shell:allow-open`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
        "crates/desktop/capabilities/main.json:6: capability grants `http:default`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
        "crates/desktop/capabilities/main.json:7: capability grants `opener:default`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
        "crates/desktop/capabilities/main.json:8: capability grants `dialog:allow-ask`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
    ]), inspects 1 => super::cases::forbidden_prefixes,
    /// A prefix in the middle of a string, and `core:plugin:fs`, grant nothing: the check passes.
    prefixes_mid_string: Passes, inspects 1 => super::cases::prefixes_mid_string,
    /// A `core:default` written with a `\u` escape fails, at the line of its key.
    escaped_value: FailsWith(&[
        "crates/desktop/capabilities/main.json:3: capability grants `core:default` (written with escapes)\n    \
         fix: list only the narrow permissions the window needs; `core:default` also grants event emit and \
         devtools toggling",
    ]), inspects 1 => super::cases::escaped_value,
    /// A `remote` key written with a `\u` escape fails at line 1, since a key has no line of its
    /// own.
    escaped_key: FailsWith(&[
        "crates/desktop/capabilities/main.json:1: capability file mentions `remote` (written with escapes)\n    \
         fix: remove the remote block, or reword the text: a remote page must never reach the IPC, and \
         `remote` is matched anywhere",
    ]), inspects 1 => super::cases::escaped_key,
    /// A `.toml` capability file is read too, and fails at the line of its string.
    toml_capability: FailsWith(&[
        "crates/desktop/capabilities/main.toml:3: capability grants `dialog:allow-ask`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
    ]), inspects 1 => super::cases::toml_capability,
    /// A `.toml` capability file that is not valid TOML fails at the parser's line, with the
    /// parser's message and the syntax fix.
    invalid_toml: FailsWith(&[
        "crates/desktop/capabilities/main.toml:2: `crates/desktop/capabilities/main.toml` is not valid \
         TOML: unclosed array, expected `]`\n    \
         fix: fix the syntax: Tauri cannot read the file either",
    ]), inspects 1 => super::cases::invalid_toml,
    /// A `.json` capability file that is not valid JSON fails at the parser's line.
    invalid_json: FailsWith(&[
        "crates/desktop/capabilities/main.json:4: `crates/desktop/capabilities/main.json` is not valid \
         JSON: expected value at line 4 column 1\n    \
         fix: fix the syntax: Tauri cannot read the file either",
    ]), inspects 1 => super::cases::invalid_json,
    /// A `.json` capability file that is not UTF-8 fails at line 1: it cannot be read as text.
    not_utf8_capability: FailsWith(&[
        "crates/desktop/capabilities/main.json:1: could not read `crates/desktop/capabilities/main.json`: \
         stream did not contain valid UTF-8\n    \
         fix: make the file readable as UTF-8 text",
    ]), inspects 1 => super::cases::not_utf8_capability,
    /// A `.json5` file that is not plain JSON fails at line 1, and the fix is to write plain JSON.
    json5_not_plain: FailsWith(&[
        "crates/desktop/capabilities/main.json5:1: json5 capability files are not supported; use .json\n    \
         fix: rename it to .json and write plain JSON: xtask cannot decode json5, so it cannot check the grants",
    ]), inspects 1 => super::cases::json5_not_plain,
    /// A `.json5` file that is plain JSON is read like a `.json` file and passes.
    plain_json5: Passes, inspects 1 => super::cases::plain_json5,
    /// An inline capability object in `tauri.conf.json` fails at the line of its string.
    inline_capability: FailsWith(&[
        "crates/desktop/tauri.conf.json:7: capability grants `http:default`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
    ]), inspects 1 => super::cases::inline_capability,
    /// A capability file in a nested subfolder is read, and reported with its repo path.
    nested_subfolder: FailsWith(&[
        "crates/desktop/capabilities/extra/local.json:3: capability grants `shell:allow-open`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
    ]), inspects 2 => super::cases::nested_subfolder,
    /// Files with other extensions (`.DS_Store`, `main.json~`) are skipped and the check passes.
    other_extensions: Passes, inspects 1 => super::cases::other_extensions,
    /// A TOML string spelling `core:default` with a `\u` escape fails at its key's line.
    escaped_toml_value: FailsWith(&[
        "crates/desktop/capabilities/main.toml:3: capability grants `core:default` (written with escapes)\n    \
         fix: list only the narrow permissions the window needs; `core:default` also grants event emit and \
         devtools toggling",
    ]), inspects 1 => super::cases::escaped_toml_value,
    /// A plain-JSON `.json5` file spelling `core:default` with a `\u` escape fails too.
    escaped_json5_value: FailsWith(&[
        "crates/desktop/capabilities/main.json5:3: capability grants `core:default` (written with escapes)\n    \
         fix: list only the narrow permissions the window needs; `core:default` also grants event emit and \
         devtools toggling",
    ]), inspects 1 => super::cases::escaped_json5_value,
    /// A TOML literal string `'core:default'` fails at its line.
    toml_literal_string: FailsWith(&[
        "crates/desktop/capabilities/main.toml:2: capability grants `core:default`\n    \
         fix: list only the narrow permissions the window needs; `core:default` also grants event emit and \
         devtools toggling",
    ]), inspects 1 => super::cases::toml_literal_string,
    /// A TOML `[remote]` table fails at the table's line.
    toml_remote_table: FailsWith(&[
        "crates/desktop/capabilities/main.toml:3: capability file mentions `remote`\n    \
         fix: remove the remote block, or reword the text: a remote page must never reach the IPC, and \
         `remote` is matched anywhere",
    ]), inspects 1 => super::cases::toml_remote_table,
    /// A scoped permission object granting `fs:allow-read` fails at its identifier.
    prefix_in_identifier_object: FailsWith(&[
        "crates/desktop/capabilities/main.json:5: capability grants `fs:allow-read`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
    ]), inspects 1 => super::cases::prefix_in_identifier_object,
    /// A file holding a list of capabilities is walked into each one.
    capability_list_file: FailsWith(&[
        "crates/desktop/capabilities/main.json:6: capability grants `opener:default`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
    ]), inspects 1 => super::cases::capability_list_file,
    /// A hidden `.extra.json`, which Tauri's glob reads, is read too.
    hidden_file: FailsWith(&[
        "crates/desktop/capabilities/.extra.json:2: capability grants `http:default`\n    \
         fix: do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission",
    ]), inspects 2 => super::cases::hidden_file,
    /// An escaped inline `core:default` in `tauri.conf.json` fails at the `capabilities` key.
    inline_escaped: FailsWith(&[
        "crates/desktop/tauri.conf.json:4: capability grants `core:default` (written with escapes)\n    \
         fix: list only the narrow permissions the window needs; `core:default` also grants event emit and \
         devtools toggling",
    ]), inspects 1 => super::cases::inline_escaped,
    /// A desktop crate moved to `apps/shell` is found by its package name, and its file is
    /// reported there.
    moved_crate: FailsWith(&[
        "apps/shell/capabilities/main.json:2: capability grants `core:default`\n    \
         fix: list only the narrow permissions the window needs; `core:default` also grants event emit and \
         devtools toggling",
    ]), inspects 1 => super::cases::moved_crate,
}
