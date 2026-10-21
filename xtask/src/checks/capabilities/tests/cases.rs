//! The builders of the planted cases of the capabilities check: each makes a scratch tree (or
//! a context) and returns what the check's own `run` finds in it. See `tests/planted.rs`.

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

/// Runs the check on the scratch repo `repo(case, files)`, with the desktop crate at its usual
/// place.
fn run_on(case: &str, files: &[(&str, &str)]) -> Inspection {
    run(&desktop_context(&repo(case, files)))
}

pub(super) fn clean_capability() -> Inspection {
    run_on(
        "clean",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"windows\": [\"main\"],\n  \
             \"permissions\": [\"core:event:default\"]\n}\n",
        )],
    )
}

pub(super) fn desktop_crate_missing() -> Inspection {
    let dir = Scratch::new("planted-capabilities-desktop-missing");
    run(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    })
}

pub(super) fn folder_missing() -> Inspection {
    run_on("folder-missing", &[])
}

pub(super) fn empty_folder() -> Inspection {
    let dir = Scratch::new("planted-capabilities-empty-folder");
    Scratch::create_dir_all(dir.join("crates/desktop/capabilities")).expect("create the folder");
    run(&desktop_context(&dir))
}

pub(super) fn core_default_value() -> Inspection {
    run_on(
        "core-default-value",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"permissions\": [\n    \"core:default\"\n  ]\n}",
        )],
    )
}

pub(super) fn core_default_key() -> Inspection {
    run_on(
        "core-default-key",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"core:default\": true\n}",
        )],
    )
}

pub(super) fn remote_key() -> Inspection {
    run_on(
        "remote-key",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"remote\": { \"urls\": [\"https://*\"] }\n}",
        )],
    )
}

pub(super) fn remote_value() -> Inspection {
    run_on(
        "remote-value",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"description\": \"never loads a remote page\"\n}",
        )],
    )
}

pub(super) fn forbidden_prefixes() -> Inspection {
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

pub(super) fn prefixes_mid_string() -> Inspection {
    run_on(
        "prefixes-mid-string",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"description\": \"refs: fs: and shell: are prose\",\n  \
             \"windows\": [\"main\"],\n  \"permissions\": [\"core:event:default\", \"core:plugin:fs\"]\n}",
        )],
    )
}

pub(super) fn escaped_value() -> Inspection {
    run_on(
        "escaped-value",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"permissions\": [\n    \"core\\u003adefault\"\n  ]\n}",
        )],
    )
}

pub(super) fn escaped_key() -> Inspection {
    run_on(
        "escaped-key",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"re\\u006dote\": []\n}",
        )],
    )
}

pub(super) fn toml_capability() -> Inspection {
    run_on(
        "toml-capability",
        &[(
            "crates/desktop/capabilities/main.toml",
            "identifier = \"main\"\nwindows = [\"main\"]\npermissions = [\"dialog:allow-ask\"]\n",
        )],
    )
}

pub(super) fn invalid_toml() -> Inspection {
    run_on(
        "invalid-toml",
        &[(
            "crates/desktop/capabilities/main.toml",
            "identifier = \"main\"\npermissions = [\n",
        )],
    )
}

pub(super) fn invalid_json() -> Inspection {
    run_on(
        "invalid-json",
        &[(
            PATH,
            "{\n  \"identifier\": \"main\",\n  \"permissions\": [\n}",
        )],
    )
}

pub(super) fn json5_not_plain() -> Inspection {
    run_on(
        "json5-not-plain",
        &[(
            "crates/desktop/capabilities/main.json5",
            "{\n  identifier: \"main\",\n  permissions: [],\n}\n",
        )],
    )
}

pub(super) fn plain_json5() -> Inspection {
    run_on(
        "plain-json5",
        &[(
            "crates/desktop/capabilities/main.json5",
            "{ \"identifier\": \"main\", \"windows\": [\"main\"], \"permissions\": [] }",
        )],
    )
}

/// The Tauri config holds one inline capability object, with `http:default` on line 7.
pub(super) fn inline_capability() -> Inspection {
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

pub(super) fn nested_subfolder() -> Inspection {
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
pub(super) fn other_extensions() -> Inspection {
    let dir = repo("other-extensions", &[(PATH, CLEAN)]);
    let caps = dir.join("crates/desktop/capabilities");
    Scratch::write(
        caps.join(".DS_Store"),
        b"\x00\x00\x00\x01Bud1\xff\xfe\x00remote",
    )
    .expect("write .DS_Store");
    Scratch::write(caps.join("main.json~"), "[\"core:default\"]").expect("write main.json~");
    run(&desktop_context(&dir))
}

/// The desktop crate is found by its package name at `apps/shell`, not at `crates/desktop`.
pub(super) fn moved_crate() -> Inspection {
    let dir = repo(
        "moved-crate",
        &[(
            "apps/shell/capabilities/main.json",
            "{\n  \"permissions\": [\"core:default\"]\n}",
        )],
    );
    let mut ctx = desktop_context(&dir);
    ctx.members.as_mut().expect("the context has members")[0].dir = "apps/shell".to_owned();
    run(&ctx)
}

/// A capability file that is not UTF-8 cannot be read as text, so it is a violation; the other
/// file still counts as inspected.
pub(super) fn not_utf8_capability() -> Inspection {
    let dir = repo(
        "not-utf8",
        &[("crates/desktop/capabilities/other.json", CLEAN)],
    );
    Scratch::write(dir.join(PATH), b"{\"identifier\": \"main\"}\xff").expect("write main.json");
    run(&desktop_context(&dir))
}
