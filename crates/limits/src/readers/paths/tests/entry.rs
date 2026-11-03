//! The SDK's entry file, which `claude` walks by the no-symlink rule after its `package.json`.

use super::{CLAUDE_SCRIPT, CLAUDE_SDK, SDK_DIR, SDK_PACKAGE, launch, rule};
use crate::readers::paths::claude;
use crate::write::scratch::Scratch;

/// `claude` on an install whose SDK `package.json` holds `package`, with each of `files` (under
/// the SDK's folder) an empty regular file: `Ok` or the rule it broke.
fn check(name: &str, package: &str, files: &[&str]) -> Result<(), String> {
    let scratch = Scratch::new(name);
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, package);
    for file in files {
        scratch.file(&format!("{SDK_DIR}/{file}"), "");
    }
    match claude(&launch(&script)) {
        Ok(_) => Ok(()),
        Err(err) => Err(rule(Err::<(), _>(err))),
    }
}

/// [`check`]'s rule, which must contain `want`.
fn refused(name: &str, package: &str, files: &[&str], want: &str) {
    match check(name, package, files) {
        Ok(()) => panic!("{name}: not refused"),
        Err(got) => assert!(got.contains(want), "{name}: {got}"),
    }
}

/// `"exports"` set to `exports`, as a whole `package.json`.
fn exports(exports: &str) -> String {
    format!(r#"{{"name":"@anthropic-ai/claude-agent-sdk","exports":{exports}}}"#)
}

#[test]
fn the_pinned_sdks_entry_must_be_a_regular_file_in_the_sdk() {
    assert_eq!(check("entry-ok", SDK_PACKAGE, &["sdk.mjs"]), Ok(()));
    refused(
        "entry-missing",
        SDK_PACKAGE,
        &[],
        "(the SDK's entry in its package.json) does not exist",
    );
    let scratch = Scratch::new("entry-dir");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    scratch.dir(&format!("{SDK_DIR}/sdk.mjs"));
    assert!(rule(claude(&launch(&script))).contains("sdk.mjs is not a regular file"));
}

#[cfg(unix)]
#[test]
fn a_linked_entry_is_refused() {
    let scratch = Scratch::new("entry-link");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    let elsewhere = scratch.file("elsewhere/sdk.mjs", "");
    let linked = scratch.link(&format!("{SDK_DIR}/sdk.mjs"), &elsewhere);
    let got = rule(claude(&launch(&script)));
    assert!(got.contains("is a link inside the install"), "{got}");
    assert!(got.contains(&linked.display().to_string()), "{got}");
}

#[test]
fn every_file_an_import_condition_names_must_pass() {
    // node takes the first matching condition, but serde_json may not keep the order, so both
    // `node` and `default` count; `require` and `types` do not match an import.
    let conditions = exports(
        r#"{".":{"types":"./t.d.ts","require":"./c.cjs","node":"./a.mjs","default":"./b.mjs"}}"#,
    );
    assert_eq!(check("cond-ok", &conditions, &["a.mjs", "b.mjs"]), Ok(()));
    refused("cond-default", &conditions, &["a.mjs"], "b.mjs");
    refused("cond-node", &conditions, &["b.mjs"], "a.mjs");
    let nested = exports(r#"{".":{"import":{"module-sync":"./m.mjs","types":"./m.d.ts"}}}"#);
    assert_eq!(check("cond-nested", &nested, &["m.mjs"]), Ok(()));
    refused("cond-nested-missing", &nested, &[], "m.mjs");
}

#[test]
fn exports_may_be_one_file_or_the_conditions_of_dot() {
    assert_eq!(
        check("short", &exports(r#""./sdk.mjs""#), &["sdk.mjs"]),
        Ok(())
    );
    let top = exports(r#"{"default":"./sdk.mjs"}"#);
    assert_eq!(check("top-conditions", &top, &["sdk.mjs"]), Ok(()));
    refused("top-conditions-missing", &top, &[], "sdk.mjs");
    // exports wins over main.
    let both = r#"{"main":"gone.mjs","exports":"./sdk.mjs"}"#;
    assert_eq!(check("exports-over-main", both, &["sdk.mjs"]), Ok(()));
}

#[test]
fn exports_that_name_no_file_are_refused() {
    let cases = [
        (
            r#"{".":"./a.mjs","default":"./b.mjs"}"#,
            "mixes subpaths and conditions",
        ),
        (r#"{"./core":"./core.mjs"}"#, "exports no \".\" entry"),
        (
            r#"{".":{"require":"./c.cjs"}}"#,
            "exports no file for an import",
        ),
        (r#"{".":null}"#, "exports no file for an import"),
        (r#"["./sdk.mjs"]"#, "has an exports array"),
        (r#"{".":{"default":["./sdk.mjs"]}}"#, "has an exports array"),
        (r#"{".":1}"#, "has an export that is not a file"),
        ("true", "has an export that is not a file"),
    ];
    for (n, (value, want)) in cases.into_iter().enumerate() {
        refused(&format!("exports-{n}"), &exports(value), &["sdk.mjs"], want);
    }
}

#[test]
fn an_entry_that_leaves_the_sdk_or_bends_the_path_is_refused() {
    let cases = [
        "sdk.mjs",
        "../sdk.mjs",
        "./../sdk.mjs",
        "./x/../../sdk.mjs",
        r"./x\sdk.mjs",
        r"./x\..\..\sdk.mjs",
        "/sdk.mjs",
        "./",
        "./x//sdk.mjs",
        "./x/./sdk.mjs",
        "./node_modules/sdk.mjs",
        "./Node_Modules/sdk.mjs",
    ];
    for (n, target) in cases.into_iter().enumerate() {
        let package = exports(&format!("{target:?}"));
        refused(
            &format!("bent-{n}"),
            &package,
            &["sdk.mjs", "x/sdk.mjs"],
            "leaves the package",
        );
    }
    let nested = exports(r#""./x/sdk.mjs""#);
    assert_eq!(check("nested-ok", &nested, &["x/sdk.mjs"]), Ok(()));
}

#[test]
fn without_exports_the_entry_is_main_or_index_js() {
    let bare = r#"{"name":"@anthropic-ai/claude-agent-sdk"}"#;
    assert_eq!(check("index", bare, &["index.js"]), Ok(()));
    refused("index-missing", bare, &["sdk.mjs"], "index.js");
    for (n, main) in ["sdk.mjs", "./sdk.mjs"].into_iter().enumerate() {
        let package = format!(r#"{{"main":"{main}"}}"#);
        assert_eq!(check(&format!("main-{n}"), &package, &["sdk.mjs"]), Ok(()));
    }
    refused(
        "main-missing",
        r#"{"main":"sdk.mjs"}"#,
        &["index.js"],
        "sdk.mjs",
    );
    refused(
        "main-up",
        r#"{"main":"../sdk.mjs"}"#,
        &["sdk.mjs"],
        "leaves the package",
    );
    refused(
        "main-number",
        r#"{"main":1}"#,
        &["sdk.mjs"],
        "has a main that is not a string",
    );
}

#[test]
fn a_package_json_that_is_not_json_or_too_large_is_refused() {
    refused("not-json", "{", &["sdk.mjs"], "is not JSON");
    // Exactly the cap reads; one byte over is refused, even when the first bytes parse.
    let short = r#"{"exports":"./sdk.mjs"}"#;
    let at_cap = format!("{short}{}", " ".repeat(1024 * 1024 - short.len()));
    assert_eq!(check("at-cap", &at_cap, &["sdk.mjs"]), Ok(()));
    let over = format!("{at_cap} ");
    refused(
        "over-cap",
        &over,
        &["sdk.mjs"],
        "is larger than 1048576 bytes",
    );
}
