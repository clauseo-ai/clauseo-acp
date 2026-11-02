//! Many links in one matter (review round 1, item 3): Seatbelt failed to compile a profile with
//! about 4,000 link rules, and 20,000 overflowed the argument list. Each test prints its
//! timings (setup, build with the walk, the jailed `echo`) for the report.

use std::time::Duration;

use clauseo_sandbox::{Clock, LINK_RULES_MAX, SystemClock};

use crate::support::{World, launcher, run};

/// Folders and files per folder: 20,000 files.
const FOLDERS: usize = 100;
const PER_FOLDER: usize = 200;

/// `<base>/d<i>/f<j>.txt` for every folder and file.
fn names(base: &str) -> impl Iterator<Item = String> + '_ {
    (0..FOLDERS).flat_map(move |i| (0..PER_FOLDER).map(move |j| format!("{base}/d{i}/f{j}.txt")))
}

/// Builds the policy and runs `/bin/echo ok` in the jail, printing how long each took.
fn build_and_start(w: &World, what: &str, setup: Duration) -> clauseo_sandbox::Policy {
    let clock = SystemClock::new();
    let policy = w.builder().build().unwrap();
    let built = clock.now();
    let (launcher, _) = launcher(policy.clone());
    let out = run(&launcher, "/bin/echo", &["ok"], &w.at("m1"), &[]);
    let started = clock.now().saturating_sub(built);
    assert_eq!(out.assert_ok().stdout, "ok\n");
    println!(
        "{what}: setup {setup:?}, build (walk) {built:?}, jailed echo {started:?}, {} findings, {} link folders",
        policy.links().len(),
        policy.link_folders().len()
    );
    policy
}

#[test]
fn twenty_thousand_hard_link_pairs_inside_the_matter_give_no_findings() {
    let w = World::new();
    let clock = SystemClock::new();
    for name in names("m1/docs") {
        w.file(&name, "doc\n");
    }
    for (name, copy) in names("m1/docs").zip(names("m1/copies")) {
        if copy.ends_with("/f0.txt") {
            w.dir(copy.rsplit_once('/').unwrap().0);
        }
        w.hard_link(&name, &copy);
    }
    let policy = build_and_start(&w, "20,000 pairs inside", clock.now());
    assert!(policy.links().is_empty(), "{:?}", &policy.links()[..3]);
    let (launcher, _) = launcher(policy);
    let out = run(
        &launcher,
        "/bin/cat",
        &["copies/d7/f9.txt"],
        &w.at("m1"),
        &[],
    );
    assert_eq!(out.assert_ok().stdout, "doc\n");
}

#[test]
fn twenty_thousand_hard_links_to_outside_files_fit_the_budget() {
    let w = World::new();
    let clock = SystemClock::new();
    for name in names("outside/src") {
        w.file(&name, "outside\n");
    }
    for (name, link) in names("outside/src").zip(names("m1/linked")) {
        if link.ends_with("/f0.txt") {
            w.dir(link.rsplit_once('/').unwrap().0);
        }
        w.hard_link(&name, &link);
    }
    w.file("m1/plain.txt", "plain\n");
    let policy = build_and_start(&w, "20,000 links outside", clock.now());
    assert_eq!(policy.links().len(), FOLDERS * PER_FOLDER);
    assert_eq!(policy.link_folders().len(), FOLDERS);
    assert!(policy.link_folders().len() <= LINK_RULES_MAX);
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let out = run(&launcher, "/bin/cat", &["linked/d42/f3.txt"], &m1, &[]);
    out.assert_refused();
    assert!(!out.stdout.contains("outside"), "{out:#?}");
    assert_eq!(
        run(&launcher, "/bin/cat", &["plain.txt"], &m1, &[])
            .assert_ok()
            .stdout,
        "plain\n"
    );
}

/// `projects/repo` (a working tree) beside many hard links to outside files: the build either
/// refuses or leaves `projects/repo/brief.txt` readable, never silently unreadable.
#[test]
fn a_repository_beside_many_links_stays_readable_or_the_build_is_refused() {
    let w = World::new();
    w.file("m1/projects/repo/.git/HEAD", "ref: refs/heads/main\n");
    w.file("m1/projects/repo/brief.txt", "brief\n");
    // The reviewer's layout: one link per folder right under `projects/`, which cannot fold.
    for i in 0..300 {
        w.file(&format!("outside/o{i}.txt"), "outside\n");
        w.dir(&format!("m1/projects/x{i}"));
        w.hard_link(&format!("outside/o{i}.txt"), &format!("m1/projects/x{i}/f"));
    }
    let error = w.builder().build().unwrap_err();
    assert!(error.to_string().contains("300 links"), "{error}");
    // One level deeper they fold into `projects/docs`, and the repository stays readable.
    for i in 0..300 {
        w.dir(&format!("m1/projects/docs/x{i}"));
        w.hard_link(
            &format!("outside/o{i}.txt"),
            &format!("m1/projects/docs/x{i}/f"),
        );
        w.remove(&format!("m1/projects/x{i}/f"));
    }
    let policy = w.builder().build().unwrap();
    assert_eq!(
        policy.link_folders().len(),
        1,
        "{:?}",
        policy.link_folders()
    );
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let out = run(
        &launcher,
        "/bin/cat",
        &["projects/repo/brief.txt"],
        &m1,
        &[],
    );
    assert_eq!(out.assert_ok().stdout, "brief\n");
    run(&launcher, "/bin/cat", &["projects/docs/x5/f"], &m1, &[]).assert_refused();
}
