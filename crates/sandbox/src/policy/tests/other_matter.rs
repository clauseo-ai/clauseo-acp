//! Reads of another matter (R18): `PolicyBuilder::other_matter_read` and the cross-matter
//! branch of `Policy::check_allowed_folder`.

use super::{World, canon, refused};
use crate::{Access, PathClass, PolicyBuilder};

/// A world with another matter, `m2`, holding a folder, a file and every refused name.
fn world(test: &str) -> World {
    let w = World::new(test);
    for name in [
        "m2/docs/sub/.git",
        "m2/.git",
        "m2/.clauseo",
        "m2/.claude",
        "m2/.codex",
        "m2/.agents",
        "m2/docs/.Claude",
    ] {
        w.dir.dir(name);
    }
    w.dir.file("m2/docs/brief.txt", "brief");
    w
}

fn with_m2(w: &World) -> PolicyBuilder {
    w.builder()
        .hidden(w.at("data"))
        .hidden_matter(&w.at("m2"), "m2-id")
}

#[test]
fn a_read_of_another_matters_folder_or_file_is_kept() {
    let w = world("other-read-ok");
    let policy = with_m2(&w)
        .other_matter_read(&w.at("m2"), false)
        .other_matter_read(&w.at("m2/docs/brief.txt"), true)
        .build()
        .unwrap();
    let reads = policy.other_matter_reads();
    assert_eq!(reads.len(), 2);
    assert_eq!(reads[0].path(), canon(&w.at("m2")));
    assert_eq!(reads[0].root(), canon(&w.at("m2")));
    assert_eq!(reads[0].matter(), "m2-id");
    assert!(!reads[0].is_file());
    assert_eq!(reads[1].path(), canon(&w.at("m2/docs/brief.txt")));
    assert!(reads[1].is_file());
    // The matter stays a hidden path; the read is no allowed folder.
    assert!(policy.hidden().contains(&canon(&w.at("m2"))));
    assert_eq!(policy.allowed_folders(), [] as [std::path::PathBuf; 0]);
    // Refused names at any depth of the grant, in any case.
    let m2 = canon(&w.at("m2"));
    for refused in [
        ".git/config",
        ".clauseo/matter.json",
        "docs/sub/.git/HEAD",
        "docs/.Claude",
    ] {
        assert!(reads[0].refuses(&m2.join(refused)), "{refused}");
    }
    assert!(!reads[0].refuses(&m2.join("docs/brief.txt")));
    assert!(!reads[0].refuses(&m2.join("docs/.github/x")));
}

#[test]
fn a_read_with_a_refused_name_anywhere_is_refused() {
    let w = world("other-read-names");
    for (rel, name) in [
        ("m2/.git", ".git"),
        ("m2/.clauseo", ".clauseo"),
        ("m2/.claude", ".claude"),
        ("m2/.codex", ".codex"),
        ("m2/.agents", ".agents"),
        ("m2/docs/sub/.git", ".git"),
        ("m2/docs/.Claude", ".Claude"),
    ] {
        let rule = format!("a read of another matter may not be or sit in a {name}");
        refused(
            with_m2(&w).other_matter_read(&w.at(rel), false).build(),
            &rule,
            &w.at(rel),
        );
    }
    // A file in one, and a link that resolves into one.
    w.dir.file("m2/.git/config", "[core]");
    let rule = "a read of another matter may not be or sit in a .git";
    refused(
        with_m2(&w)
            .other_matter_read(&w.at("m2/.git/config"), true)
            .build(),
        rule,
        &w.at("m2/.git/config"),
    );
    #[cfg(unix)]
    {
        w.dir.symlink("m2/docs/cfg", &w.at("m2/.git/config"));
        let result = with_m2(&w)
            .other_matter_read(&w.at("m2/docs/cfg"), true)
            .build();
        refused(result, rule, &w.at("m2/.git/config"));
    }
}

#[test]
fn a_read_outside_another_matter_or_holding_a_hidden_path_is_refused() {
    let w = world("other-read-where");
    let what = "a read of another matter";
    // Not in a matter: the data dir, a plain folder.
    w.dir.dir("data/agents");
    for rel in ["data/agents", "other"] {
        refused(
            with_m2(&w).other_matter_read(&w.at(rel), false).build(),
            &format!("{what} must be in another matter's folder"),
            &w.at(rel),
        );
    }
    // Holding a hidden path, or in a matter inside one.
    w.dir.dir("m2/docs/secret");
    refused(
        with_m2(&w)
            .hidden(w.at("m2/docs/secret"))
            .other_matter_read(&w.at("m2/docs"), false)
            .build(),
        &format!("{what} may not hold a hidden path"),
        &w.at("m2/docs"),
    );
    w.dir.dir("data/m3/docs");
    w.dir.dir("data/m3/.clauseo");
    refused(
        with_m2(&w)
            .hidden_matter(&w.at("data/m3"), "m3-id")
            .other_matter_read(&w.at("data/m3/docs"), false)
            .build(),
        &format!("{what} may not be in a matter inside another hidden path"),
        &w.at("data/m3/docs"),
    );
    // A file granted as a tree, and a folder as a file.
    refused(
        with_m2(&w)
            .other_matter_read(&w.at("m2/docs/brief.txt"), false)
            .build(),
        &format!("{what} must be a folder, as granted"),
        &w.at("m2/docs/brief.txt"),
    );
    refused(
        with_m2(&w)
            .other_matter_read(&w.at("m2/docs"), true)
            .build(),
        &format!("{what} must be a file, as granted"),
        &w.at("m2/docs"),
    );
}

#[test]
fn an_allowed_folder_in_another_matter_is_still_refused() {
    let w = world("other-read-allowed");
    refused(
        with_m2(&w).allowed_folder(w.at("m2/docs")).build(),
        "an allowed folder may not equal, hold or sit inside a hidden path",
        &w.at("m2/docs"),
    );
}

#[test]
fn check_allowed_folder_has_a_cross_matter_branch() {
    let w = world("other-read-check");
    let policy = with_m2(&w).build().unwrap();
    assert!(policy.check_allowed_folder(&w.at("m2")).is_ok());
    assert!(
        policy
            .check_allowed_folder(&w.at("m2/docs/brief.txt"))
            .is_ok()
    );
    for rel in ["m2/.git", "m2/.clauseo", "m2/docs/sub/.git"] {
        let refusal = policy.check_allowed_folder(&w.at(rel)).unwrap_err();
        assert!(
            refusal.to_string().contains("may not be or sit in a"),
            "{refusal}"
        );
    }
    // The data dir is a hidden path but no matter: the old refusal.
    let refusal = policy.check_allowed_folder(&w.at("data")).unwrap_err();
    assert!(refusal.to_string().contains("hidden path"), "{refusal}");
}

#[test]
fn refused_paths_in_another_matter_classify_as_that_matter() {
    let w = world("other-read-classify");
    let policy = with_m2(&w)
        .other_matter_read(&w.at("m2/docs"), false)
        .build()
        .unwrap();
    let m2 = canon(&w.at("m2"));
    let other = PathClass::OtherMatter {
        matter: "m2-id".to_owned(),
    };
    for (rel, access) in [
        ("docs/brief.txt", Access::Write),
        ("docs/sub/.git/HEAD", Access::Read),
        (".clauseo/matter.json", Access::Read),
        ("elsewhere.txt", Access::Read),
    ] {
        assert_eq!(policy.classify(&m2.join(rel), access), other, "{rel}");
    }
    assert_eq!(policy.hidden_matter_of(&m2.join("docs")), Some("m2-id"));
    let data = canon(&w.at("data")).join("x");
    assert_eq!(policy.classify(&data, Access::Read), PathClass::Hidden);
}
