//! The Claude store and the browser socket folder are write grants, like the notes and temp
//! folders: neither may be inside the matter or hold it, or the agent could write there.

use super::*;

#[test]
fn the_claude_store_may_not_be_inside_the_matter_or_hold_it() {
    let w = World::new("store-matter");
    let rule = "the Claude store may not be inside the matter or hold it";
    w.dir.dir("matter/store");
    refused(
        w.builder().claude_store(w.at("matter/store")).build(),
        rule,
        &w.at("matter/store"),
    );
    refused(
        w.builder().claude_store(w.at("matter")).build(),
        rule,
        &w.at("matter"),
    );
    // A folder that holds the matter: the temp folder the world sits in.
    refused(
        w.builder().claude_store(w.dir.path()).build(),
        rule,
        w.dir.path(),
    );
    let policy = w.builder().claude_store(w.at("other")).build().unwrap();
    let store = policy.claude_store().unwrap();
    assert_eq!(store.dir(), canon(&w.at("other")));
}

#[cfg(unix)]
#[test]
fn the_browser_socket_folder_may_not_be_inside_the_matter_or_hold_it() {
    use crate::BrowserSocketDir;
    let w = World::new("socket-matter");
    let rule = "the browser socket folder may not be inside the matter or hold it";
    let inside = BrowserSocketDir::create_in(&w.at("matter")).unwrap();
    refused(
        w.builder().browser_socket_dir(&inside).build(),
        rule,
        inside.path(),
    );
    // A socket folder that holds the matter: the matter sits inside it.
    let holding = BrowserSocketDir::create_in(w.dir.path()).unwrap();
    let matter = w.dir.dir(&format!("clauseo-ab-{}/matter", holding.id()));
    refused(
        Policy::builder(&matter, w.at("state"))
            .home(w.at("home"))
            .browser_socket_dir(&holding)
            .build(),
        rule,
        holding.path(),
    );
    let outside = BrowserSocketDir::create_in(&w.at("other")).unwrap();
    let policy = w.builder().browser_socket_dir(&outside).build().unwrap();
    assert_eq!(policy.browser_socket_dir(), Some(outside.canonical()));
}
