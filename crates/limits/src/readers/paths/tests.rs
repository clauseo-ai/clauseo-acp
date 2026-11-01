//! The install folder and the no-symlink rule, on real folders under a scratch folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clauseo_settings::AgentLaunch;

use super::{
    CLAUDE_HELPER, CLAUDE_ID, CODEX_ID, Located, check_run_dir, claude, codex_js, has_dot_part,
    locate,
};
use crate::error::Error;
use crate::write::scratch::Scratch;

const CLAUDE_SCRIPT: &str =
    "install/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js";
const CODEX_SCRIPT: &str = "install/node_modules/@zed-industries/codex-acp/bin/codex-acp.js";
const CLAUDE_SDK: &str = "install/node_modules/@anthropic-ai/claude-agent-sdk/package.json";
const SDK_PACKAGE: &str = r#"{"name":"@anthropic-ai/claude-agent-sdk","version":"0.3.293"}"#;

fn node() -> String {
    if cfg!(windows) {
        r"C:\node\node.exe".to_owned()
    } else {
        "/usr/local/bin/node".to_owned()
    }
}

fn launch(script: &Path) -> AgentLaunch {
    AgentLaunch {
        command: node(),
        args: vec![script.to_string_lossy().into_owned(), "--extra".to_owned()],
        env: BTreeMap::new(),
    }
}

fn rule(result: Result<impl std::fmt::Debug, Error>) -> String {
    match result {
        Err(Error::Refused { rule, .. }) => rule,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn the_install_folder_is_above_the_nearest_node_modules() {
    let scratch = Scratch::new("paths-ok");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    let located = locate(CLAUDE_ID, &launch(&script)).unwrap();
    assert_eq!(
        located,
        Located {
            program: PathBuf::from(node()),
            install: scratch.0.join("install"),
            script: script.clone(),
        }
    );
    let (_, helper) = claude(&launch(&script)).unwrap();
    assert_eq!(helper, scratch.0.join("install").join(CLAUDE_HELPER));

    let nested = scratch.file("outer/node_modules/a/node_modules/b/index.js", "");
    let located = locate(CLAUDE_ID, &launch(&nested)).unwrap();
    assert_eq!(located.install, scratch.0.join("outer/node_modules/a"));
}

#[test]
fn settings_that_break_a_rule_are_refused_by_name() {
    let scratch = Scratch::new("paths-rules");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    let relative_command = AgentLaunch {
        command: "node".to_owned(),
        ..launch(&script)
    };
    assert!(
        rule(locate(CLAUDE_ID, &relative_command)).contains("(command) is not an absolute path")
    );
    let no_args = AgentLaunch {
        args: Vec::new(),
        ..launch(&script)
    };
    assert!(rule(locate(CLAUDE_ID, &no_args)).contains("args is empty"));
    let relative = launch(Path::new("node_modules/x/index.js"));
    assert!(rule(locate(CLAUDE_ID, &relative)).contains("(args[0]) is not an absolute path"));
    let dotted = launch(
        &scratch
            .0
            .join("install/node_modules/../node_modules/x/index.js"),
    );
    assert!(rule(locate(CLAUDE_ID, &dotted)).contains("has a . or .. part"));
    let no_modules = scratch.file("loose/dist/index.js", "");
    assert!(rule(locate(CLAUDE_ID, &launch(&no_modules))).contains("no node_modules folder"));
    let missing = scratch.0.join("install/node_modules/gone/index.js");
    assert!(rule(locate(CLAUDE_ID, &launch(&missing))).contains("(args[0]) does not exist"));
    let folder = scratch.dir("install/node_modules/folder/index.js");
    assert!(rule(locate(CLAUDE_ID, &launch(&folder))).contains("is not a regular file"));
    let message = locate(CLAUDE_ID, &relative_command)
        .unwrap_err()
        .to_string();
    assert!(message.starts_with("agents.claude-acp: "), "{message}");
}

#[test]
fn dot_parts_are_found_as_written() {
    assert!(has_dot_part(Path::new("/a/./b")));
    assert!(has_dot_part(Path::new("/a/../b")));
    assert!(has_dot_part(Path::new("./a")));
    assert!(!has_dot_part(Path::new("/a/.b/c..d/...")));
    assert_eq!(has_dot_part(Path::new(r"C:\a\..\b")), cfg!(windows));
}

#[test]
fn something_at_the_helper_path_must_be_a_regular_file() {
    let scratch = Scratch::new("paths-helper");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    scratch.file(&format!("install/{CLAUDE_HELPER}"), "old helper");
    assert!(claude(&launch(&script)).is_ok());
    let other = Scratch::new("paths-helper-dir");
    let script = other.file(CLAUDE_SCRIPT, "");
    other.file(CLAUDE_SDK, SDK_PACKAGE);
    other.dir(&format!("install/{CLAUDE_HELPER}"));
    assert!(rule(claude(&launch(&script))).contains("exists and is not a regular file"));
}

#[cfg(unix)]
#[test]
fn a_link_under_the_install_folder_is_refused_and_above_it_allowed() {
    let scratch = Scratch::new("paths-links");
    let real = scratch.file("store/claude-agent-acp/dist/index.js", "");
    let package = real.parent().unwrap().parent().unwrap();
    scratch.link(
        "install/node_modules/@agentclientprotocol/claude-agent-acp",
        package,
    );
    let through_link = scratch.0.join(CLAUDE_SCRIPT);
    assert!(
        rule(locate(CLAUDE_ID, &launch(&through_link))).contains("is a link inside the install")
    );

    let linked_file = scratch.link("install2/node_modules/pkg/index.js", &real);
    assert!(
        rule(locate(CLAUDE_ID, &launch(&linked_file))).contains("is a link inside the install")
    );

    let target = scratch.dir("real-install");
    scratch.file("real-install/node_modules/pkg/index.js", "");
    let linked_install = scratch.link("linked-install", &target);
    let script = linked_install.join("node_modules/pkg/index.js");
    assert_eq!(
        locate(CLAUDE_ID, &launch(&script)).unwrap().install,
        linked_install
    );
}

#[cfg(unix)]
#[test]
fn a_link_at_the_helper_path_is_refused() {
    let scratch = Scratch::new("paths-helper-link");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    let elsewhere = scratch.file("elsewhere.mjs", "keep me");
    scratch.link(&format!("install/{CLAUDE_HELPER}"), &elsewhere);
    assert!(rule(claude(&launch(&script))).contains("exists and is not a regular file"));
}

#[test]
fn claude_needs_the_sdk_as_a_regular_file_in_its_install_folder() {
    let scratch = Scratch::new("paths-sdk");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    // An SDK in a parent's node_modules, which node would climb to, does not count.
    scratch.file(
        "node_modules/@anthropic-ai/claude-agent-sdk/package.json",
        SDK_PACKAGE,
    );
    let missing = rule(claude(&launch(&script)));
    assert!(
        missing.contains("no @anthropic-ai/claude-agent-sdk/package.json in"),
        "{missing}"
    );
    scratch.dir(CLAUDE_SDK);
    assert!(rule(claude(&launch(&script))).contains("is not a regular file"));
    let other = Scratch::new("paths-sdk-ok");
    let script = other.file(CLAUDE_SCRIPT, "");
    other.file(CLAUDE_SDK, SDK_PACKAGE);
    assert!(claude(&launch(&script)).is_ok());
}

#[cfg(unix)]
#[test]
fn a_linked_sdk_is_refused() {
    let scratch = Scratch::new("paths-sdk-link");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    let real = scratch.dir("store/claude-agent-sdk");
    scratch.file("store/claude-agent-sdk/package.json", SDK_PACKAGE);
    scratch.link("install/node_modules/@anthropic-ai/claude-agent-sdk", &real);
    assert!(rule(claude(&launch(&script))).contains("is a link inside the install"));
}

#[test]
fn codex_js_is_codex_acps_own_first_then_the_install_folders() {
    let scratch = Scratch::new("paths-codex");
    let script = scratch.file(CODEX_SCRIPT, "");
    let shared = scratch.file("install/node_modules/@openai/codex/bin/codex.js", "");
    let (located, found) = codex_js(&launch(&script)).unwrap();
    assert_eq!(located.install, scratch.0.join("install"));
    assert_eq!(found, shared);
    let own = scratch.file(
        "install/node_modules/@zed-industries/codex-acp/node_modules/@openai/codex/bin/codex.js",
        "",
    );
    assert_eq!(codex_js(&launch(&script)).unwrap().1, own);
}

#[test]
fn no_codex_js_is_refused() {
    let scratch = Scratch::new("paths-codex-none");
    let script = scratch.file(CODEX_SCRIPT, "");
    let err = codex_js(&launch(&script)).unwrap_err();
    assert!(
        matches!(
            err,
            Error::Refused {
                agent: CODEX_ID,
                ..
            }
        ),
        "{err}"
    );
    assert!(
        err.to_string().contains("no @openai/codex/bin/codex.js"),
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn a_linked_codex_package_is_refused_not_skipped() {
    let scratch = Scratch::new("paths-codex-link");
    let script = scratch.file(CODEX_SCRIPT, "");
    scratch.file("install/node_modules/@openai/codex/bin/codex.js", "");
    let real = scratch.dir("store/codex");
    scratch.file("store/codex/bin/codex.js", "");
    scratch.link(
        "install/node_modules/@zed-industries/codex-acp/node_modules/@openai/codex",
        &real,
    );
    assert!(rule(codex_js(&launch(&script))).contains("is a link inside the install"));
}

#[test]
fn the_run_folder_may_be_missing_or_a_folder_but_nothing_else() {
    let scratch = Scratch::new("paths-run");
    let run = scratch.0.join("data/limits/run");
    assert!(check_run_dir(&run).is_ok());
    scratch.dir("data/limits/run");
    assert!(check_run_dir(&run).is_ok());
    let other = Scratch::new("paths-run-file");
    other.file("data/limits", "");
    let err = check_run_dir(&other.0.join("data/limits/run")).unwrap_err();
    assert!(matches!(err, Error::RunFolder { .. }), "{err}");
    assert!(err.to_string().ends_with("is not a folder"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_linked_run_folder_or_limits_folder_is_refused() {
    let scratch = Scratch::new("paths-run-link");
    let elsewhere = scratch.dir("elsewhere");
    scratch.dir("data/limits");
    scratch.link("data/limits/run", &elsewhere);
    let err = check_run_dir(&scratch.0.join("data/limits/run")).unwrap_err();
    assert!(err.to_string().ends_with("is a link"), "{err}");
    scratch.link("data2/limits", &elsewhere);
    let err = check_run_dir(&scratch.0.join("data2/limits/run")).unwrap_err();
    assert!(err.to_string().contains("data2/limits is a link"), "{err}");
}
