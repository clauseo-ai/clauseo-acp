//! The session's temp folder, the agent's `TMPDIR`: `<data_dir>/run/<session>/tmp` when the
//! host sets it, else the state folder's `tmp` (the deprecated fallback).

use super::*;

/// The world with `data/run/s1`, the session's run folder, made.
fn with_run(test: &str) -> World {
    let w = World::new(test);
    w.dir.dir("data/run/s1");
    w
}

#[test]
fn the_set_temp_folder_is_the_tmpdir_and_need_not_exist() {
    let w = with_run("tmpdir-set");
    let policy = w
        .builder()
        .hidden(w.at("data"))
        .tmpdir(w.at("data/run/s1/tmp"))
        .build()
        .unwrap();
    let want = canon(&w.at("data/run/s1")).join("tmp");
    assert_eq!(policy.tmpdir(), want);
    assert_eq!(policy.session_tmpdir(), Some(want.as_path()));
    // An existing folder there is fine too.
    w.dir.dir("data/run/s1/tmp");
    let policy = w.builder().tmpdir(w.at("data/run/s1/tmp")).build().unwrap();
    assert_eq!(policy.tmpdir(), canon(&w.at("data/run/s1/tmp")));
}

#[test]
fn without_one_the_tmpdir_falls_back_to_the_state_folders_tmp() {
    let w = World::new("tmpdir-fallback");
    let policy = w.builder().build().unwrap();
    assert_eq!(policy.tmpdir(), canon(&w.at("state")).join("tmp"));
    assert_eq!(policy.session_tmpdir(), None);
}

#[test]
fn the_temp_folder_is_allowed_again_and_its_run_folder_stays_hidden() {
    let w = with_run("tmpdir-classify");
    let policy = w
        .builder()
        .hidden(w.at("data"))
        .tmpdir(w.at("data/run/s1/tmp"))
        .build()
        .unwrap();
    let run = canon(&w.at("data/run/s1"));
    let inside = run.join("tmp/scratch.txt");
    assert_eq!(policy.classify(&inside, Access::Write), PathClass::Outside);
    let token = run.join("token");
    assert_eq!(policy.classify(&token, Access::Read), PathClass::Hidden);
    assert_eq!(policy.classify(&token, Access::Write), PathClass::Hidden);
}

#[test]
fn a_temp_folder_on_or_over_the_home_or_state_folder_is_refused() {
    let w = with_run("tmpdir-home-state");
    refused(
        w.builder().tmpdir(w.at("home")).build(),
        "the temp folder may not equal or hold the home folder",
        &w.at("home"),
    );
    refused(
        w.builder().tmpdir(w.dir.path()).build(),
        "the temp folder may not equal or hold the home folder",
        w.dir.path(),
    );
    w.dir.dir("data/state");
    refused(
        Policy::builder(w.at("matter"), w.at("data/state"))
            .home(w.at("home"))
            .tmpdir(w.at("data"))
            .build(),
        "the temp folder may not equal or hold the state folder",
        &w.at("data"),
    );
    refused(
        w.builder().tmpdir(w.at("state")).build(),
        "the temp folder may not equal or hold the state folder",
        &w.at("state"),
    );
    // Inside the state folder is the fallback's own place, and fine.
    w.builder().tmpdir(w.at("state/tmp")).build().unwrap();
}

#[test]
fn a_temp_folder_in_the_matter_over_a_hidden_path_or_relative_is_refused() {
    let w = with_run("tmpdir-matter-hidden");
    refused(
        w.builder().tmpdir(w.at("matter/tmp")).build(),
        "the temp folder may not be inside the matter or hold it",
        &w.at("matter/tmp"),
    );
    refused(
        w.builder()
            .hidden(w.at("data/run/s1/tmp/secret"))
            .tmpdir(w.at("data/run/s1/tmp"))
            .build(),
        "the temp folder may not equal or hold a hidden path",
        &w.at("data/run/s1/tmp"),
    );
    refused(
        w.builder().tmpdir("run/s1/tmp").build(),
        "the temp folder must be an absolute path",
        Path::new("run/s1/tmp"),
    );
    refused(
        w.builder().tmpdir(w.at("data/run/s2/tmp")).build(),
        "the temp folder must exist",
        &w.at("data/run/s2/tmp"),
    );
}

#[cfg(unix)]
#[test]
fn a_temp_folder_that_is_a_symbolic_link_is_refused() {
    let w = with_run("tmpdir-link");
    let link = w.dir.symlink("data/run/s1/tmp", &w.at("other"));
    refused(
        w.builder().tmpdir(&link).build(),
        "the temp folder may not be a symbolic link",
        &link,
    );
    // A dangling one too: the launcher would make the folder through it.
    let dangling = w.dir.symlink("data/run/s1/gone", &w.at("nowhere"));
    refused(
        w.builder().tmpdir(&dangling).build(),
        "the temp folder may not be a symbolic link",
        &dangling,
    );
    // A link above the folder is fine: only its own name is checked.
    w.dir.dir("data/run/s2");
    let above = w.dir.symlink("above", w.dir.path());
    w.builder()
        .tmpdir(above.join("data/run/s2/tmp"))
        .build()
        .unwrap();
}
