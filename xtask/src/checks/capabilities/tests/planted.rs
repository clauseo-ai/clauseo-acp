//! The planted cases of the capabilities check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). Their builders are in `tests/repos.rs`.

use crate::checks::capabilities::Fix;
use crate::checks::planted::{fails, failure};

crate::checks::planted::cases! {
    /// A capability file that grants nothing forbidden passes.
    clean_capability: Passes, inspects 1 => super::repos::clean_capability,
    /// A workspace with no desktop crate stops at the root manifest, and the zero line says so.
    desktop_crate_missing: Fails[fails(
        "Cargo.toml:1",
        "the desktop crate (package `clauseo-desktop`) is not a workspace member",
        crate::checks::Fix::DesktopNotMember,
    )], inspects 0 => super::repos::desktop_crate_missing,
    /// A desktop crate with no capabilities folder fails at line 1 and stops the check.
    folder_missing: Fails[fails(
        "crates/desktop/capabilities:1",
        "the capabilities folder `crates/desktop/capabilities` is missing",
        Fix::FolderMissing,
    )], inspects 0 => super::repos::folder_missing,
    /// An empty capabilities folder inspects nothing, and the zero line names the normal fix.
    empty_folder: Fails[failure(
        "0 capability and permission files inspected in the desktop crate's capabilities/ and \
         permissions/",
        crate::checks::Fix::NothingInspected {
            name: "capabilities",
            many: "capability and permission files",
            place: "in the desktop crate's capabilities/ and permissions/".to_owned(),
            source: "xtask/src/checks/capabilities.rs",
        },
    )], inspects 0 => super::repos::empty_folder,
    /// `core:default` as a string value fails at the line of that string.
    core_default_value: Fails[fails(
        "crates/desktop/capabilities/main.json:4",
        "capability grants `core:default`",
        Fix::CoreDefault,
    )], inspects 1 => super::repos::core_default_value,
    /// `core:default` as a key fails at the line of that key.
    core_default_key: Fails[fails(
        "crates/desktop/capabilities/main.json:3",
        "capability grants `core:default`",
        Fix::CoreDefault,
    )], inspects 1 => super::repos::core_default_key,
    /// `remote` as a key fails at the line of that key.
    remote_key: Fails[fails(
        "crates/desktop/capabilities/main.json:3",
        "capability file mentions `remote`",
        Fix::Remote,
    )], inspects 1 => super::repos::remote_key,
    /// `remote` in prose, as a string value, fails and says how to reword it.
    remote_value: Fails[fails(
        "crates/desktop/capabilities/main.json:3",
        "capability file mentions `remote`",
        Fix::Remote,
    )], inspects 1 => super::repos::remote_value,
    /// Each forbidden prefix (`fs:`, `shell:`, `http:`, `opener:`, `dialog:`) fails on its own
    /// line.
    forbidden_prefixes: Fails[
        fails(
            "crates/desktop/capabilities/main.json:4",
            "capability grants `fs:allow-read`",
            Fix::ForbiddenPrefix,
        ),
        fails(
            "crates/desktop/capabilities/main.json:5",
            "capability grants `shell:allow-open`",
            Fix::ForbiddenPrefix,
        ),
        fails(
            "crates/desktop/capabilities/main.json:6",
            "capability grants `http:default`",
            Fix::ForbiddenPrefix,
        ),
        fails(
            "crates/desktop/capabilities/main.json:7",
            "capability grants `opener:default`",
            Fix::ForbiddenPrefix,
        ),
        fails(
            "crates/desktop/capabilities/main.json:8",
            "capability grants `dialog:allow-ask`",
            Fix::ForbiddenPrefix,
        ),
    ], inspects 1 => super::repos::forbidden_prefixes,
    /// A prefix in the middle of a string, and `core:plugin:fs`, grant nothing: the check passes.
    prefixes_mid_string: Passes, inspects 1 => super::repos::prefixes_mid_string,
    /// A `core:default` written with a `\u` escape fails, at the line of its key.
    escaped_value: Fails[fails(
        "crates/desktop/capabilities/main.json:3",
        "capability grants `core:default` (written with escapes)",
        Fix::CoreDefault,
    )], inspects 1 => super::repos::escaped_value,
    /// A `remote` key written with a `\u` escape fails at line 1, since a key has no line of its
    /// own.
    escaped_key: Fails[fails(
        "crates/desktop/capabilities/main.json:1",
        "capability file mentions `remote` (written with escapes)",
        Fix::Remote,
    )], inspects 1 => super::repos::escaped_key,
    /// A `.toml` capability file is read too, and fails at the line of its string.
    toml_capability: Fails[fails(
        "crates/desktop/capabilities/main.toml:3",
        "capability grants `dialog:allow-ask`",
        Fix::ForbiddenPrefix,
    )], inspects 1 => super::repos::toml_capability,
    /// A `.toml` capability file that is not valid TOML fails at the parser's line, with the
    /// parser's message and the syntax fix.
    invalid_toml: Fails[fails(
        "crates/desktop/capabilities/main.toml:2",
        "`crates/desktop/capabilities/main.toml` is not valid TOML: unclosed array, expected `]`",
        Fix::Syntax,
    )], inspects 1 => super::repos::invalid_toml,
    /// A `.json` capability file that is not valid JSON fails at the parser's line.
    invalid_json: Fails[fails(
        "crates/desktop/capabilities/main.json:4",
        "`crates/desktop/capabilities/main.json` is not valid JSON: expected value at line 4 \
         column 1",
        Fix::Syntax,
    )], inspects 1 => super::repos::invalid_json,
    /// A `.json` capability file that is not UTF-8 fails at line 1: it cannot be read as text.
    not_utf8_capability: Fails[fails(
        "crates/desktop/capabilities/main.json:1",
        "could not read `crates/desktop/capabilities/main.json`: stream did not contain valid \
         UTF-8",
        crate::checks::Fix::NotUtf8,
    )], inspects 1 => super::repos::not_utf8_capability,
    /// A `.json5` file that is not plain JSON fails at line 1, and the fix is to write plain JSON.
    json5_not_plain: Fails[fails(
        "crates/desktop/capabilities/main.json5:1",
        "json5 capability files are not supported; use .json",
        Fix::Json5,
    )], inspects 1 => super::repos::json5_not_plain,
    /// A `.json5` file that is plain JSON is read like a `.json` file and passes.
    plain_json5: Passes, inspects 1 => super::repos::plain_json5,
    /// An inline capability object in `tauri.conf.json` fails at the line of its string.
    inline_capability: Fails[fails(
        "crates/desktop/tauri.conf.json:7",
        "capability grants `http:default`",
        Fix::ForbiddenPrefix,
    )], inspects 1 => super::repos::inline_capability,
    /// A capability file in a nested subfolder is read, and reported with its repo path.
    nested_subfolder: Fails[fails(
        "crates/desktop/capabilities/extra/local.json:3",
        "capability grants `shell:allow-open`",
        Fix::ForbiddenPrefix,
    )], inspects 2 => super::repos::nested_subfolder,
    /// Files with other extensions (`.DS_Store`, `main.json~`) are skipped and the check passes.
    other_extensions: Passes, inspects 1 => super::repos::other_extensions,
    /// A TOML string spelling `core:default` with a `\u` escape fails at its key's line.
    escaped_toml_value: Fails[fails(
        "crates/desktop/capabilities/main.toml:3",
        "capability grants `core:default` (written with escapes)",
        Fix::CoreDefault,
    )], inspects 1 => super::repos::escaped_toml_value,
    /// A plain-JSON `.json5` file spelling `core:default` with a `\u` escape fails too.
    escaped_json5_value: Fails[fails(
        "crates/desktop/capabilities/main.json5:3",
        "capability grants `core:default` (written with escapes)",
        Fix::CoreDefault,
    )], inspects 1 => super::repos::escaped_json5_value,
    /// A TOML literal string `'core:default'` fails at its line.
    toml_literal_string: Fails[fails(
        "crates/desktop/capabilities/main.toml:2",
        "capability grants `core:default`",
        Fix::CoreDefault,
    )], inspects 1 => super::repos::toml_literal_string,
    /// A TOML `[remote]` table fails at the table's line.
    toml_remote_table: Fails[fails(
        "crates/desktop/capabilities/main.toml:3",
        "capability file mentions `remote`",
        Fix::Remote,
    )], inspects 1 => super::repos::toml_remote_table,
    /// A scoped permission object granting `fs:allow-read` fails at its identifier.
    prefix_in_identifier_object: Fails[fails(
        "crates/desktop/capabilities/main.json:5",
        "capability grants `fs:allow-read`",
        Fix::ForbiddenPrefix,
    )], inspects 1 => super::repos::prefix_in_identifier_object,
    /// A file holding a list of capabilities is walked into each one.
    capability_list_file: Fails[fails(
        "crates/desktop/capabilities/main.json:6",
        "capability grants `opener:default`",
        Fix::ForbiddenPrefix,
    )], inspects 1 => super::repos::capability_list_file,
    /// A hidden `.extra.json`, which Tauri's glob reads, is read too.
    hidden_file: Fails[fails(
        "crates/desktop/capabilities/.extra.json:2",
        "capability grants `http:default`",
        Fix::ForbiddenPrefix,
    )], inspects 2 => super::repos::hidden_file,
    /// An escaped inline `core:default` in `tauri.conf.json` fails at the `capabilities` key.
    inline_escaped: Fails[fails(
        "crates/desktop/tauri.conf.json:4",
        "capability grants `core:default` (written with escapes)",
        Fix::CoreDefault,
    )], inspects 1 => super::repos::inline_escaped,
    /// A desktop crate moved to `apps/shell` is found by its package name, and its file is
    /// reported there.
    moved_crate: Fails[fails(
        "apps/shell/capabilities/main.json:2",
        "capability grants `core:default`",
        Fix::CoreDefault,
    )], inspects 1 => super::repos::moved_crate,
    /// A permission set bundling `fs:allow-read`, which a capability grants as `"all"`, fails at
    /// the set's line: the capability file and the permission file are both read.
    permission_set_bundles_a_forbidden_permission: Fails[fails(
        "crates/desktop/permissions/x.toml:3",
        "permission file names `fs:allow-read`",
        Fix::ForbiddenPrefix,
    )], inspects 2 => super::repos::permission_set_bundles_a_forbidden_permission,
    /// A JSON permission file in a subfolder whose default names `core:default` with a `\u`
    /// escape fails at its key's line, and one mentioning `remote` fails with the reword fix.
    permission_json_escaped_and_remote: Fails[
        fails(
            "crates/desktop/permissions/nested/.hidden.json:3",
            "permission file names `core:default` (written with escapes)",
            Fix::CoreDefault,
        ),
        fails(
            "crates/desktop/permissions/nested/.hidden.json:6",
            "permission file mentions `remote`",
            Fix::RemotePermission,
        ),
    ], inspects 2 => super::repos::permission_json_escaped_and_remote,
    /// Permission files beside a capabilities folder with no capability file fail that folder,
    /// and are still read: the one clean permission file is counted.
    permissions_without_a_capability_file: Fails[fails(
        "crates/desktop/capabilities:1",
        "the capabilities folder `crates/desktop/capabilities` holds no capability file",
        Fix::NoCapabilityFile,
    )], inspects 1 => super::repos::permissions_without_a_capability_file,
    /// A clean permission file passes; a schema in `permissions/schemas/` and a `.json5` file,
    /// which Tauri never reads as permissions, are skipped, though they name `fs:` permissions.
    clean_permission_file: Passes, inspects 2 => super::repos::clean_permission_file,
    /// A capability file that is a symlink to a file is read through the link: the `core:default`
    /// in the file it names fails at the link's path, on the line of its string.
    file_symlink: Fails[fails(
        "crates/desktop/capabilities/main.json:3",
        "capability grants `core:default`",
        Fix::CoreDefault,
    )], inspects 1 => super::repos::file_symlink,
    /// `capabilities/x -> ..` fails and the check finishes: the walk does not follow the link,
    /// which Tauri's glob would follow up into the crate's folder.
    symlink_to_a_folder_above: Fails[fails(
        "crates/desktop/capabilities/x:1",
        "`crates/desktop/capabilities/x` is a symlink to a folder: Tauri reads the files behind \
         it, and this check does not follow it",
        Fix::LinkedFolder,
    )], inspects 1 => super::repos::symlink_to_a_folder_above,
    /// A symlink in `permissions/` to a folder whose file grants `fs:default` fails at the link.
    permissions_symlink_to_a_folder: Fails[fails(
        "crates/desktop/permissions/sets:1",
        "`crates/desktop/permissions/sets` is a symlink to a folder: Tauri reads the files behind \
         it, and this check does not follow it",
        Fix::LinkedFolder,
    )], inspects 1 => super::repos::permissions_symlink_to_a_folder,
}
