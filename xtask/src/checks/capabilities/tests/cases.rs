//! The builders of the planted cases of the capabilities check: each makes a scratch tree (or
//! a context) that the planted harness runs the check on. See `tests/planted.rs`.

use super::*;

/// A scratch repo named after `case`, with each `(path, text)` of `files` written into it.
fn repo(case: &str, files: &[(&str, &str)]) -> Scratch {
    let dir = Scratch::new(&format!("planted-capabilities-{case}"));
    for (path, text) in files {
        let full = dir.join(path);
        Scratch::create_dir_all(full.parent().expect("a file has a parent"))
            .expect("create the dirs");
        Scratch::write(full, text).expect("write the file");
    }
    dir
}

/// The scratch repo `repo(case, files)`, with the desktop crate at its usual
/// place.
fn run_on(case: &str, files: &[(&str, &str)]) -> Repo {
    Repo::desktop(repo(case, files))
}

pub(super) fn clean_capability() -> Repo {
    run_on(
        "clean",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"windows\": [\"main\"],\n  \
             \"permissions\": [\"core:event:default\"]\n}\n",
        )],
    )
}

pub(super) fn desktop_crate_missing() -> Repo {
    let dir = Scratch::new("planted-capabilities-desktop-missing");
    Repo::new(dir)
}

pub(super) fn folder_missing() -> Repo {
    run_on("folder-missing", &[])
}

pub(super) fn empty_folder() -> Repo {
    let dir = Scratch::new("planted-capabilities-empty-folder");
    Scratch::create_dir_all(dir.join("crates/desktop/capabilities")).expect("create the folder");
    Repo::desktop(dir)
}

pub(super) fn core_default_value() -> Repo {
    run_on(
        "core-default-value",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"permissions\": [\n    \"core:default\"\n  ]\n}",
        )],
    )
}

pub(super) fn core_default_key() -> Repo {
    run_on(
        "core-default-key",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"core:default\": true\n}",
        )],
    )
}

pub(super) fn remote_key() -> Repo {
    run_on(
        "remote-key",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"remote\": { \"urls\": [\"https://*\"] }\n}",
        )],
    )
}

pub(super) fn remote_value() -> Repo {
    run_on(
        "remote-value",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"description\": \"never loads a remote page\"\n}",
        )],
    )
}

pub(super) fn forbidden_prefixes() -> Repo {
    run_on(
        "forbidden-prefixes",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"permissions\": [\n    \"fs:allow-read\",\n    \
             \"shell:allow-open\",\n    \"http:default\",\n    \"opener:default\",\n    \
             \"dialog:allow-ask\"\n  ]\n}",
        )],
    )
}

pub(super) fn prefixes_mid_string() -> Repo {
    run_on(
        "prefixes-mid-string",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"description\": \"refs: fs: and shell: are prose\",\n  \
             \"windows\": [\"main\"],\n  \"permissions\": [\"core:event:default\", \"core:plugin:fs\"]\n}",
        )],
    )
}

pub(super) fn escaped_value() -> Repo {
    run_on(
        "escaped-value",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"permissions\": [\n    \"core\\u003adefault\"\n  ]\n}",
        )],
    )
}

pub(super) fn escaped_key() -> Repo {
    run_on(
        "escaped-key",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"re\\u006dote\": []\n}",
        )],
    )
}

pub(super) fn toml_capability() -> Repo {
    run_on(
        "toml-capability",
        &[(
            "crates/desktop/capabilities/main.toml",
            "identifier = \"main\"\nwindows = [\"main\"]\npermissions = [\"dialog:allow-ask\"]\n",
        )],
    )
}

pub(super) fn invalid_toml() -> Repo {
    run_on(
        "invalid-toml",
        &[(
            "crates/desktop/capabilities/main.toml",
            "identifier = \"main\"\npermissions = [\n",
        )],
    )
}

pub(super) fn invalid_json() -> Repo {
    run_on(
        "invalid-json",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"permissions\": [\n}",
        )],
    )
}

pub(super) fn json5_not_plain() -> Repo {
    run_on(
        "json5-not-plain",
        &[(
            "crates/desktop/capabilities/main.json5",
            "{\n  identifier: \"main\",\n  permissions: [],\n}\n",
        )],
    )
}

pub(super) fn plain_json5() -> Repo {
    run_on(
        "plain-json5",
        &[(
            "crates/desktop/capabilities/main.json5",
            "{ \"identifier\": \"main\", \"windows\": [\"main\"], \"permissions\": [] }",
        )],
    )
}

/// The Tauri config holds one inline capability object, with `http:default` on line 7.
pub(super) fn inline_capability() -> Repo {
    run_on(
        "inline-capability",
        &[
            (PATH, CLEAN),
            (
                "crates/desktop/tauri.conf.json",
                "{\n  \"app\": {\n    \"security\": {\n      \"capabilities\": [\n        {\n          \"identifier\": \"inline\",\n          \"permissions\": [\"http:default\"]\n        }\n      ]\n    }\n  }\n}",
            ),
        ],
    )
}

pub(super) fn nested_subfolder() -> Repo {
    run_on(
        "nested-subfolder",
        &[
            (PATH, CLEAN),
            (
                "crates/desktop/capabilities/extra/local.json",
                "{\n  \"permissions\": [\n    \"shell:allow-open\"\n  ]\n}",
            ),
        ],
    )
}

/// A Finder `.DS_Store` and a Vim backup `main.json~` sit beside a clean `main.json`: neither is
/// read, so the check passes on the one file it inspects.
pub(super) fn other_extensions() -> Repo {
    let dir = repo("other-extensions", &[(PATH, CLEAN)]);
    let caps = dir.join("crates/desktop/capabilities");
    Scratch::write(
        caps.join(".DS_Store"),
        b"\x00\x00\x00\x01Bud1\xff\xfe\x00remote",
    )
    .expect("write .DS_Store");
    Scratch::write(caps.join("main.json~"), "[\"core:default\"]").expect("write main.json~");
    Repo::desktop(dir)
}

/// The desktop crate is found by its package name at `apps/shell`, not at `crates/desktop`.
pub(super) fn moved_crate() -> Repo {
    let dir = repo(
        "moved-crate",
        &[(
            "apps/shell/capabilities/main.json",
            "{\n  \"permissions\": [\"core:default\"]\n}",
        )],
    );
    Repo::with_context(dir, |root| {
        let mut ctx = desktop_context(root);
        ctx.members.as_mut().expect("the context has members")[0].dir = "apps/shell".to_owned();
        ctx
    })
}

/// A capability file that is not UTF-8 cannot be read as text, so it is a violation; the other
/// file still counts as inspected.
pub(super) fn not_utf8_capability() -> Repo {
    let dir = repo(
        "not-utf8",
        &[("crates/desktop/capabilities/other.json", CLEAN)],
    );
    Scratch::write(dir.join(PATH), b"{\"identifier\": \"main\"}\xff").expect("write main.json");
    Repo::desktop(dir)
}

/// A TOML basic string that spells `core:default` with a `\u` escape, under `permissions` on
/// line 3.
pub(super) fn escaped_toml_value() -> Repo {
    run_on(
        "escaped-toml-value",
        &[(
            "crates/desktop/capabilities/main.toml",
            "identifier = \"main\"\nwindows = [\"main\"]\npermissions = [\"core:\\u0064efault\"]\n",
        )],
    )
}

/// A `.json5` file that is plain JSON and spells `core:default` with a `\u` escape.
pub(super) fn escaped_json5_value() -> Repo {
    run_on(
        "escaped-json5-value",
        &[(
            "crates/desktop/capabilities/main.json5",
            "{\n  \"identifier\": \"main\",\n  \"permissions\": [\"core:\\u0064efault\"]\n}",
        )],
    )
}

/// A TOML literal string `'core:default'` on line 2.
pub(super) fn toml_literal_string() -> Repo {
    run_on(
        "toml-literal-string",
        &[(
            "crates/desktop/capabilities/main.toml",
            "identifier = 'main'\npermissions = ['core:default']\n",
        )],
    )
}

/// A `[remote]` table with its URLs, on line 3.
pub(super) fn toml_remote_table() -> Repo {
    run_on(
        "toml-remote-table",
        &[(
            "crates/desktop/capabilities/main.toml",
            "identifier = \"main\"\n\n[remote]\nurls = [\"https://*.example.com\"]\n",
        )],
    )
}

/// A scoped permission object whose `identifier` is `fs:allow-read`, on line 5.
pub(super) fn prefix_in_identifier_object() -> Repo {
    run_on(
        "prefix-in-identifier-object",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"permissions\": [\n    {\n      \"identifier\": \"fs:allow-read\",\n      \"allow\": [{ \"path\": \"$HOME/**\" }]\n    }\n  ]\n}",
        )],
    )
}

/// A file holding a list of capabilities under `capabilities`, one granting `opener:default` on
/// line 6.
pub(super) fn capability_list_file() -> Repo {
    run_on(
        "capability-list-file",
        &[(
            PATH,
            "{\n  \"capabilities\": [\n    { \"identifier\": \"a\", \"permissions\": [] },\n    {\n      \"identifier\": \"b\",\n      \"permissions\": [\"opener:default\"]\n    }\n  ]\n}",
        )],
    )
}

/// A hidden `.extra.json` beside `main.json`: Tauri's `capabilities/**/*` glob matches it.
pub(super) fn hidden_file() -> Repo {
    run_on(
        "hidden-file",
        &[
            (PATH, CLEAN),
            (
                "crates/desktop/capabilities/.extra.json",
                "{\n  \"permissions\": [\"http:default\"]\n}",
            ),
        ],
    )
}

/// An inline capability in `tauri.conf.json` that spells `core:default` with a `\u` escape: it
/// points at the `capabilities` key, line 4.
pub(super) fn inline_escaped() -> Repo {
    run_on(
        "inline-escaped",
        &[
            (PATH, CLEAN),
            (
                "crates/desktop/tauri.conf.json",
                "{\n  \"app\": {\n    \"security\": {\n      \"capabilities\": [\n        { \"identifier\": \"inline\", \"permissions\": [\"core:\\u0064efault\"] }\n      ]\n    }\n  }\n}",
            ),
        ],
    )
}
