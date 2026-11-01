//! Settings paths a reader refuses: each read fails with `LaunchFailed` naming the rule, nothing
//! is launched, and nothing is written.

use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_protocol::AgentKind;
use futures::executor::block_on;

use crate::harness::{Play, Setup, SharedTimer, TestLauncher};

/// Reads Claude's limits with the settings in `setup`, expecting a refusal: returns its message
/// after checking nothing was launched and no run folder was made.
fn refused(setup: &Setup) -> String {
    let launcher = TestLauncher::new(Play::Scenario("claude-ok"));
    let reader = setup.readers(&launcher, &SharedTimer::default());
    let outcome = block_on(reader.read(AgentKind::Claude));
    let ReadOutcome::Failed(ReadFailure::LaunchFailed(message)) = outcome else {
        panic!("not a refusal: {outcome:?}");
    };
    assert_eq!(launcher.specs(), [], "nothing is launched");
    assert!(!setup.run_dir().exists(), "no run folder is made");
    message
}

#[test]
fn a_dot_dot_part_in_args_0_is_refused() {
    let setup = Setup::new();
    let script = setup.install().join(
        "node_modules/@agentclientprotocol/claude-agent-acp/../claude-agent-acp/dist/index.js",
    );
    setup.settings(&script);
    let message = refused(&setup);
    assert!(message.contains("has a . or .. part"), "{message}");
    assert!(!setup.helper().exists());
}

#[test]
fn no_node_modules_above_args_0_is_refused() {
    let setup = Setup::new();
    let script = setup.root.join("loose/dist/index.js");
    Setup::file(&script, "");
    setup.settings(&script);
    let message = refused(&setup);
    assert!(message.contains("no node_modules folder"), "{message}");
}

#[test]
fn no_claude_entry_is_refused() {
    let setup = Setup::new();
    let message = refused(&setup);
    assert_eq!(message, "no settings entry for claude-acp");
}

#[cfg(unix)]
#[test]
fn a_linked_package_folder_is_refused_and_nothing_written() {
    let setup = Setup::new();
    let real = setup.root.join("store/claude-agent-acp");
    Setup::file(&real.join("dist/index.js"), "");
    let package = setup
        .install()
        .join("node_modules/@agentclientprotocol/linked-acp");
    crate::write::symlink(&real, &package).unwrap();
    setup.settings(&package.join("dist/index.js"));
    let message = refused(&setup);
    assert!(
        message.contains("is a link inside the install folder"),
        "{message}"
    );
    assert!(!message.contains(crate::harness::FAKE_KEY), "{message}");
    assert!(!setup.helper().exists(), "no helper is written");
    assert_eq!(Setup::names(&real), ["dist"]);
}

#[cfg(unix)]
#[test]
fn a_link_at_the_helper_path_is_refused_and_its_target_untouched() {
    let setup = Setup::new();
    setup.settings(&setup.script());
    let target = setup.root.join("elsewhere.mjs");
    Setup::file(&target, "keep me\n");
    crate::write::symlink(&target, &setup.helper()).unwrap();
    let message = refused(&setup);
    assert!(
        message.contains("exists and is not a regular file"),
        "{message}"
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
    assert!(
        std::fs::symlink_metadata(setup.helper())
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn a_file_at_the_limits_folder_is_refused() {
    let setup = Setup::new();
    setup.settings(&setup.script());
    Setup::file(&setup.data().join("limits"), "not a folder\n");
    let message = refused(&setup);
    assert!(message.contains("is not a folder"), "{message}");
    assert!(!setup.helper().exists(), "no helper is written");
}

#[cfg(unix)]
#[test]
fn a_linked_run_folder_is_refused_and_its_target_untouched() {
    let setup = Setup::new();
    setup.settings(&setup.script());
    let elsewhere = setup.root.join("elsewhere");
    crate::write::create_dir_all(&elsewhere).unwrap();
    crate::write::create_dir_all(&setup.data().join("limits")).unwrap();
    crate::write::symlink(&elsewhere, &setup.run_dir()).unwrap();
    let launcher = TestLauncher::new(Play::Scenario("claude-ok"));
    let reader = setup.readers(&launcher, &SharedTimer::default());
    let outcome = block_on(reader.read(AgentKind::Claude));
    let ReadOutcome::Failed(ReadFailure::LaunchFailed(message)) = outcome else {
        panic!("not a refusal: {outcome:?}");
    };
    assert!(message.contains("is a link"), "{message}");
    assert_eq!(launcher.specs(), []);
    assert_eq!(Setup::names(&elsewhere), Vec::<String>::new());
}
