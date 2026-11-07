//! The allowed sites' views and their file: a view follows every publish, a matter sees its own
//! allows and the `Always` ones, overrides off empties every view, dead views are dropped, and
//! the file round-trips, a damaged one counting as no allows.

use std::collections::BTreeSet;

use clauseo_protocol::{AllowScope, MatterId, Timestamp};
use uuid::Uuid;

use super::AllowedSites;
use super::file::{self, Stored};
use crate::pipeline::tests::support::TempDir;

fn matter() -> MatterId {
    MatterId::new(Uuid::now_v7()).unwrap()
}

fn at(millis: i64) -> Timestamp {
    Timestamp::new(millis).unwrap()
}

fn set(hosts: &[&str]) -> BTreeSet<String> {
    hosts.iter().map(|host| (*host).to_owned()).collect()
}

#[test]
fn a_view_follows_each_publish_for_its_matter_only() {
    let (a, b) = (matter(), matter());
    let sites = AllowedSites::default();
    let a_view = sites.for_matter(a);
    assert!(std::sync::Arc::ptr_eq(&a_view, &sites.for_matter(a)));
    let b_view = sites.for_matter(b);
    let mut stored = Stored::default();
    assert!(stored.allow("evil.example", AllowScope::Matter, a, at(1)));
    assert!(stored.allow("phish.test", AllowScope::Always, b, at(2)));
    assert!(!stored.allow("phish.test", AllowScope::Always, a, at(3)));
    sites.publish(stored.in_force(true));
    assert_eq!(*a_view.get(), set(&["evil.example", "phish.test"]));
    assert_eq!(*b_view.get(), set(&["phish.test"]));
    // A view taken after the publish starts from it.
    let c_view = sites.for_matter(matter());
    assert_eq!(*c_view.get(), set(&["phish.test"]));
    // Overrides off: every view empty, nothing deleted.
    sites.publish(stored.in_force(false));
    assert!(a_view.get().is_empty() && b_view.get().is_empty());
    assert_eq!(stored.list().len(), 2);
    // Dropped views go at the next publish.
    drop((b_view, c_view));
    sites.publish(stored.in_force(true));
    assert_eq!(sites.views(), 1);
}

#[test]
fn a_revoke_takes_back_one_scope_only_and_lists_sort_always_first() {
    let (a, b) = (matter(), matter());
    let mut stored = Stored::default();
    stored.allow("evil.example", AllowScope::Matter, a, at(1));
    stored.allow("evil.example", AllowScope::Matter, b, at(2));
    stored.allow("evil.example", AllowScope::Always, a, at(3));
    assert!(!stored.revoke("evil.example", AllowScope::Matter, None));
    assert!(stored.revoke("evil.example", AllowScope::Matter, Some(a)));
    assert!(!stored.revoke("evil.example", AllowScope::Matter, Some(a)));
    let listed: Vec<_> = stored
        .list()
        .into_iter()
        .map(|site| (site.scope, site.matter, site.allowed_at))
        .collect();
    assert_eq!(
        listed,
        [
            (AllowScope::Always, None, at(3)),
            (AllowScope::Matter, Some(b), at(2))
        ]
    );
    assert!(stored.revoke("evil.example", AllowScope::Always, None));
    assert_eq!(stored.in_force(true).always, BTreeSet::new());
}

#[test]
fn the_file_round_trips_and_a_damaged_one_counts_as_no_allows() {
    let dir = TempDir::new("allowed-sites-file");
    let path = file::path(dir.path());
    assert_eq!(file::read(&path), Stored::default());
    let mut stored = Stored::default();
    let a = matter();
    stored.allow("evil.example", AllowScope::Matter, a, at(1));
    stored.allow("2606:4700::1111", AllowScope::Always, a, at(2));
    file::write(&path, &stored).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        format!(
            r#"{{"v":1,"always":{{"2606:4700::1111":2}},"matters":{{"{a}":{{"evil.example":1}}}}}}"#
        )
    );
    assert_eq!(file::read(&path), stored);
    for bad in [
        r#"{"v":2,"always":{}}"#,
        "not json",
        r#"{"v":1,"always":[]}"#,
    ] {
        crate::write::replace_private_file(&path, bad.as_bytes()).unwrap();
        assert_eq!(file::read(&path), Stored::default(), "{bad}");
    }
}

/// A file over the 1 MiB cap counts as no allows, even when it parses; one at the cap reads.
#[test]
fn a_file_over_the_cap_counts_as_no_allows() {
    let dir = TempDir::new("allowed-sites-cap");
    let path = file::path(dir.path());
    let mut stored = Stored::default();
    stored.allow("evil.example", AllowScope::Always, matter(), at(1));
    file::write(&path, &stored).unwrap();
    let json = std::fs::read(&path).unwrap();
    let cap = usize::try_from(file::CAP).unwrap();
    let mut at_cap = json.clone();
    at_cap.resize(cap, b' ');
    crate::write::replace_private_file(&path, &at_cap).unwrap();
    assert_eq!(file::read(&path), stored);
    let mut over = json;
    over.resize(cap + 1, b' ');
    crate::write::replace_private_file(&path, &over).unwrap();
    assert_eq!(file::read(&path), Stored::default());
}

/// Past `MAX_HOSTS` allows, `always` first and then by matter, the rest are ignored.
#[test]
fn allows_past_the_most_are_ignored() {
    let dir = TempDir::new("allowed-sites-most");
    let path = file::path(dir.path());
    let mut stored = Stored::default();
    let a = matter();
    for n in 0..file::MAX_HOSTS {
        stored.allow(&format!("h{n:05}.test"), AllowScope::Always, a, at(1));
    }
    stored.allow("evil.example", AllowScope::Matter, a, at(1));
    file::write(&path, &stored).unwrap();
    let read = file::read(&path);
    assert_eq!(read.list().len(), file::MAX_HOSTS);
    assert!(read.in_force(true).matters.is_empty());
    stored.revoke("h09999.test", AllowScope::Always, None);
    file::write(&path, &stored).unwrap();
    assert_eq!(file::read(&path), stored);
}
