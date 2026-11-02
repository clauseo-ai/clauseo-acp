//! The allowed-folder rules: `Policy::check_allowed_folder` and its run in `build`.

use std::path::Path;

use super::{World, canon, refused};
use crate::{Error, Policy};

#[test]
fn allowed_folders_that_meet_a_hidden_path_are_refused() {
    let w = World::new("allowed-hidden");
    let rule = "an allowed folder may not equal, hold or sit inside a hidden path";
    let data = w.at("data");
    w.dir.dir("data/agents");
    // Equal (the data dir), holds (a folder holding the data dir), inside (a folder in it).
    refused(
        w.builder().hidden(&data).allowed_folder(&data).build(),
        rule,
        &data,
    );
    w.dir.dir("outer/data2");
    refused(
        w.builder()
            .hidden(w.at("outer/data2"))
            .allowed_folder(w.at("outer"))
            .build(),
        rule,
        &w.at("outer"),
    );
    refused(
        w.builder()
            .hidden(&data)
            .allowed_folder(w.at("data/agents"))
            .build(),
        rule,
        &w.at("data/agents"),
    );
    // Another matter and a refused folder are hidden paths too.
    w.dir.dir("matter2");
    refused(
        w.builder()
            .hidden(w.at("matter2"))
            .allowed_folder(w.at("matter2"))
            .build(),
        rule,
        &w.at("matter2"),
    );
}

#[test]
fn allowed_folders_that_meet_the_matter_are_refused() {
    let w = World::new("allowed-matter");
    let rule = "an allowed folder may not equal, hold or sit inside the matter";
    refused(
        w.builder().allowed_folder(w.at("matter")).build(),
        rule,
        &w.at("matter"),
    );
    w.dir.dir("matter/sub");
    refused(
        w.builder().allowed_folder(w.at("matter/sub")).build(),
        rule,
        &w.at("matter/sub"),
    );
    // `Documents/Clauseo`, the parent of new matters, is refused because it holds the matter.
    w.dir.dir("Documents/Clauseo/Smith v Jones");
    refused(
        Policy::builder(w.at("Documents/Clauseo/Smith v Jones"), w.at("state"))
            .home(w.at("home"))
            .allowed_folder(w.at("Documents/Clauseo"))
            .build(),
        rule,
        &w.at("Documents/Clauseo"),
    );
}

#[test]
fn allowed_folders_that_are_home_root_or_git_are_refused() {
    let w = World::new("allowed-home");
    let root = w.root();
    let home_rule = "an allowed folder may not equal or hold the home folder";
    refused(
        w.builder().allowed_folder(w.at("home")).build(),
        home_rule,
        &w.at("home"),
    );
    refused(
        w.builder().allowed_folder(w.dir.path()).build(),
        home_rule,
        w.dir.path(),
    );
    refused(
        w.builder().allowed_folder(&root).build(),
        "an allowed folder may not be the root folder",
        &root,
    );
    w.dir.dir("other/repo/.git/objects");
    refused(
        w.builder()
            .allowed_folder(w.at("other/repo/.git/objects"))
            .build(),
        "an allowed folder may not be in a .git",
        &w.at("other/repo/.git/objects"),
    );
}

#[test]
fn a_good_allowed_folder_is_kept_and_check_canonicalises_first() {
    let w = World::new("allowed-ok");
    w.dir.dir("home/Precedents");
    let policy = w
        .builder()
        .hidden(w.at("data"))
        .allowed_folder(w.at("home/Precedents"))
        .build()
        .unwrap();
    assert_eq!(policy.allowed_folders(), [canon(&w.at("home/Precedents"))]);
    // `other/../data` is the data dir once resolved.
    let sneaky = w.at("other/../data");
    match policy.check_allowed_folder(&sneaky) {
        Err(Error::Policy(message)) => assert!(message.contains("hidden path"), "{message}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(policy.check_allowed_folder(&w.at("other")).is_ok());
    assert!(matches!(
        policy.check_allowed_folder(Path::new("relative")),
        Err(Error::Policy(_))
    ));
}
