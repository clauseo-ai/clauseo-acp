//! The Claude store and the browser socket folder are write grants, like the notes and temp
//! folders: neither may be inside the matter or hold it, or the agent could write there. A
//! grant also may not equal or hold the home folder, the state folder or a hidden path.

use super::*;

/// The world's `data` folder with the home and state folders in it (neither is the matter), so a
/// grant there holds them and is refused for that alone.
fn with_data(test: &str) -> World {
    let w = World::new(test);
    w.dir.dir("data/home");
    w.dir.dir("data/state");
    w
}

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

#[test]
fn a_claude_store_named_like_the_matter_but_outside_it_is_accepted() {
    let w = World::new("store-sibling");
    w.dir.dir("matter2");
    let policy = w.builder().claude_store(w.at("matter2")).build().unwrap();
    assert_eq!(
        policy.claude_store().unwrap().dir(),
        canon(&w.at("matter2"))
    );
}

#[cfg(unix)]
#[test]
fn a_claude_store_that_links_into_the_matter_is_refused() {
    let w = World::new("store-link");
    w.dir.dir("matter/real");
    let link = w.dir.symlink("other/store-link", &w.at("matter/real"));
    refused(
        w.builder().claude_store(&link).build(),
        "the Claude store may not be inside the matter or hold it",
        &w.at("matter/real"),
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_claude_store_in_the_matter_in_another_case_is_refused() {
    let w = World::new("store-case");
    w.dir.dir("matter/store");
    // The matter's folder in another case is the same folder on this volume.
    refused(
        w.builder().claude_store(w.at("Matter/store")).build(),
        "the Claude store may not be inside the matter or hold it",
        &w.at("Matter/store"),
    );
}

#[test]
fn a_claude_store_that_is_or_holds_the_home_folder_is_refused() {
    let w = with_data("store-home");
    refused(
        w.builder().claude_store(w.at("home")).build(),
        "the Claude store may not equal or hold the home folder",
        &w.at("home"),
    );
    refused(
        Policy::builder(w.at("matter"), w.at("state"))
            .home(w.at("data/home"))
            .claude_store(w.at("data"))
            .build(),
        "the Claude store may not equal or hold the home folder",
        &w.at("data"),
    );
}

#[test]
fn a_claude_store_that_is_or_holds_the_state_folder_is_refused() {
    let w = with_data("store-state");
    refused(
        Policy::builder(w.at("matter"), w.at("data/state"))
            .home(w.at("home"))
            .claude_store(w.at("data/state"))
            .build(),
        "the Claude store may not equal or hold the state folder",
        &w.at("data/state"),
    );
    refused(
        Policy::builder(w.at("matter"), w.at("data/state"))
            .home(w.at("home"))
            .claude_store(w.at("data"))
            .build(),
        "the Claude store may not equal or hold the state folder",
        &w.at("data"),
    );
}

#[test]
fn a_claude_store_that_holds_a_hidden_path_is_refused() {
    let w = with_data("store-hidden");
    refused(
        w.builder()
            .hidden(w.at("data/secret"))
            .claude_store(w.at("data"))
            .build(),
        "the Claude store may not equal or hold a hidden path",
        &w.at("data/secret"),
    );
}

#[cfg(unix)]
mod socket {
    use super::*;
    use crate::BrowserSocketDir;

    const RULE: &str = "the browser socket folder may not be inside the matter or hold it";

    #[test]
    fn the_browser_socket_folder_may_not_be_inside_the_matter_or_hold_it() {
        let w = World::new("socket-matter");
        let inside = BrowserSocketDir::create_in(&w.at("matter")).unwrap();
        refused(
            w.builder().browser_socket_dir(&inside).build(),
            RULE,
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
            RULE,
            holding.path(),
        );
        let outside = BrowserSocketDir::create_in(&w.at("other")).unwrap();
        let policy = w.builder().browser_socket_dir(&outside).build().unwrap();
        assert_eq!(policy.browser_socket_dir(), Some(outside.canonical()));
    }

    #[test]
    fn a_browser_socket_folder_named_like_the_matter_but_outside_it_is_accepted() {
        let w = World::new("socket-sibling");
        w.dir.dir("matter2");
        let socket = BrowserSocketDir::create_in(&w.at("matter2")).unwrap();
        let policy = w.builder().browser_socket_dir(&socket).build().unwrap();
        assert_eq!(policy.browser_socket_dir(), Some(socket.canonical()));
    }

    #[test]
    fn a_browser_socket_folder_that_links_into_the_matter_is_refused() {
        let w = World::new("socket-link");
        w.dir.symlink("other/to-matter", &w.at("matter"));
        // The folder is made through the link, so it lands in the matter.
        let via = BrowserSocketDir::create_in(&w.at("other/to-matter")).unwrap();
        refused(
            w.builder().browser_socket_dir(&via).build(),
            RULE,
            via.path(),
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_browser_socket_folder_in_the_matter_in_another_case_is_refused() {
        let w = World::new("socket-case");
        let socket = BrowserSocketDir::create_in(&w.at("Matter")).unwrap();
        refused(
            w.builder().browser_socket_dir(&socket).build(),
            RULE,
            socket.path(),
        );
    }

    #[test]
    fn a_browser_socket_folder_that_holds_the_home_folder_is_refused() {
        let w = World::new("socket-home");
        let socket = BrowserSocketDir::create_in(w.dir.path()).unwrap();
        let home = w.dir.dir(&format!("clauseo-ab-{}/home", socket.id()));
        refused(
            Policy::builder(w.at("matter"), w.at("state"))
                .home(&home)
                .browser_socket_dir(&socket)
                .build(),
            "the browser socket folder may not equal or hold the home folder",
            socket.path(),
        );
    }

    #[test]
    fn a_browser_socket_folder_that_holds_the_state_folder_is_refused() {
        let w = World::new("socket-state");
        let socket = BrowserSocketDir::create_in(w.dir.path()).unwrap();
        let state = w.dir.dir(&format!("clauseo-ab-{}/state", socket.id()));
        refused(
            Policy::builder(w.at("matter"), &state)
                .home(w.at("home"))
                .browser_socket_dir(&socket)
                .build(),
            "the browser socket folder may not equal or hold the state folder",
            socket.path(),
        );
    }

    #[test]
    fn a_browser_socket_folder_that_holds_a_hidden_path_is_refused() {
        let w = World::new("socket-hidden");
        let socket = BrowserSocketDir::create_in(w.dir.path()).unwrap();
        let secret = socket.path().join("secret");
        refused(
            w.builder()
                .hidden(&secret)
                .browser_socket_dir(&socket)
                .build(),
            "the browser socket folder may not equal or hold a hidden path",
            socket.path(),
        );
    }
}
