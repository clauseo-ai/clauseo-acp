//! The Windows browser order and its two reasons for none, modelled on any OS.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{
    Choice, DEVTOOLS_VALUE, Folders, POLICY_VALUE, Unavailable, Vendor, blocked_by, choose,
};

const EDGE_X86: &str = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
const EDGE: &str = r"C:\Program Files\Microsoft\Edge\Application\msedge.exe";
const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
const CHROME_X86: &str = r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe";

fn dirs() -> Folders {
    Folders {
        x86: Some(PathBuf::from(r"C:\Program Files (x86)")),
        native: Some(PathBuf::from(r"C:\Program Files")),
        local_app_data: Some(PathBuf::from(r"C:\Users\a\AppData\Local")),
        any_host_choice: false,
    }
}

/// `choose` over a machine holding `files`, with `blocked` vendors.
fn on(host: &str, files: &[&str], blocked: &[Vendor]) -> Choice {
    let files: BTreeSet<String> = files.iter().map(|f| f.to_ascii_lowercase()).collect();
    let is_file = |path: &Path| {
        let text = path
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        files.contains(&text)
    };
    choose(Path::new(host), &dirs(), is_file, |vendor| {
        blocked.contains(&vendor)
    })
}

/// `list` as Windows writes it, without case: a test on Linux joins with `/`.
fn paths(list: &[&str]) -> Vec<String> {
    list.iter()
        .map(|path| path.replace('/', "\\").to_ascii_lowercase())
        .collect()
}

fn found(list: &[PathBuf]) -> Vec<String> {
    list.iter()
        .map(|path| {
            path.to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase()
        })
        .collect()
}

#[test]
fn edge_comes_first_then_program_files_chrome() {
    let all = [EDGE_X86, EDGE, CHROME, CHROME_X86];
    let choice = on(EDGE_X86, &all, &[]);
    assert_eq!(found(&choice.try_in_order), paths(&all));
    assert_eq!(choice.unavailable(), None);
}

#[test]
fn the_hosts_choice_goes_first_once() {
    let host = r"c:\program files\google\chrome\application\CHROME.EXE";
    let choice = on(host, &[EDGE_X86, CHROME], &[]);
    assert_eq!(found(&choice.try_in_order), paths(&[host, EDGE_X86]));
}

#[test]
fn a_blocked_edge_falls_back_to_chrome() {
    let choice = on(EDGE_X86, &[EDGE_X86, CHROME], &[Vendor::Edge]);
    assert_eq!(found(&choice.try_in_order), paths(&[CHROME]));
    assert_eq!(found(&choice.blocked), paths(&[EDGE_X86]));
    assert_eq!(choice.unavailable(), None);
}

#[test]
fn nothing_installed_and_everything_blocked_are_told_apart() {
    let none = on(EDGE_X86, &[], &[]);
    assert_eq!(none.unavailable(), Some(Unavailable::NotInstalled));
    let blocked = on(
        EDGE_X86,
        &[EDGE_X86, CHROME],
        &[Vendor::Edge, Vendor::Chrome],
    );
    assert_eq!(
        blocked.unavailable(),
        Some(Unavailable::Blocked {
            edge: true,
            chrome: true
        })
    );
    let only_edge_blocked = on(EDGE_X86, &[EDGE_X86], &[Vendor::Edge]);
    assert_eq!(
        only_edge_blocked.unavailable(),
        Some(Unavailable::Blocked {
            edge: true,
            chrome: false
        })
    );
    let user = r"C:\Users\a\AppData\Local\Google\Chrome\Application\chrome.exe";
    let per_user = on(EDGE_X86, &[user], &[]);
    assert_eq!(per_user.unavailable(), Some(Unavailable::PerUserOnly));
    let per_user_and_blocked = on(EDGE_X86, &[user, EDGE_X86], &[Vendor::Edge]);
    assert_eq!(
        per_user_and_blocked.unavailable(),
        Some(Unavailable::Blocked {
            edge: true,
            chrome: false
        }),
        "the firm's block is what the lawyer can act on"
    );
    for reason in [
        Unavailable::NotInstalled,
        Unavailable::PerUserOnly,
        Unavailable::Blocked {
            edge: true,
            chrome: false,
        },
        Unavailable::Blocked {
            edge: false,
            chrome: true,
        },
        Unavailable::Blocked {
            edge: true,
            chrome: true,
        },
    ] {
        assert!(reason.agent_note().contains("Tell the lawyer"));
        assert!(reason.reason().starts_with("no browser to start"));
    }
}

#[test]
fn chrome_for_testing_and_a_per_user_chrome_are_never_tried() {
    let cft = r"C:\Users\a\AppData\Roaming\Clauseo\browsers\chrome-157\chrome-win64\chrome.exe";
    let user = r"C:\Users\a\AppData\Local\Google\Chrome\Application\chrome.exe";
    let choice = on(EDGE_X86, &[cft, user, EDGE_X86], &[]);
    assert_eq!(found(&choice.try_in_order), paths(&[EDGE_X86]));
    assert_eq!(
        choice.per_user.as_deref().map(|p| found(&[p.to_owned()])),
        Some(paths(&[user]))
    );
    for host in [cft, user] {
        let choice = on(host, &[cft, user, EDGE_X86], &[]);
        assert_eq!(
            found(&choice.try_in_order),
            paths(&[EDGE_X86]),
            "the host's choice outside Program Files is refused"
        );
        assert_eq!(choice.refused.as_deref(), Some(Path::new(host)));
    }
}

#[test]
fn only_a_host_choice_inside_program_files_is_taken() {
    for inside in [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"c:\program files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\130.0\msedge.exe",
    ] {
        let choice = on(inside, &[inside], &[]);
        assert_eq!(found(&choice.try_in_order), paths(&[inside]), "{inside}");
        assert_eq!(choice.refused, None);
    }
    for outside in [
        r"C:\Program Files Evil\chrome.exe",
        r"C:\Program Files\..\Users\a\chrome.exe",
        r"C:\Program Files",
        r"D:\Program Files\Google\Chrome\Application\chrome.exe",
        r"\\server\share\Program Files\chrome.exe",
        r"chrome.exe",
    ] {
        let choice = on(outside, &[outside], &[]);
        assert!(choice.try_in_order.is_empty(), "{outside}");
        assert_eq!(choice.refused.as_deref(), Some(Path::new(outside)));
        assert_eq!(choice.unavailable(), Some(Unavailable::NotInstalled));
    }
    let tests_only = Folders {
        any_host_choice: true,
        ..dirs()
    };
    let stub = Path::new(r"C:\scratch\chrome-stub.exe");
    let choice = choose(stub, &tests_only, |_| true, |_| false);
    assert_eq!(
        choice.try_in_order.first().map(PathBuf::as_path),
        Some(stub)
    );
}

#[test]
fn the_last_browser_that_started_is_tried_first() {
    let mut choice = on(EDGE_X86, &[EDGE_X86, CHROME], &[]);
    choice.prefer(Path::new(CHROME));
    assert_eq!(found(&choice.try_in_order), paths(&[CHROME, EDGE_X86]));
}

#[test]
fn the_machines_policy_wins_over_the_users() {
    let rd = |machine, user| blocked_by([machine, user], [None, None]);
    assert!(!rd(None, None));
    assert!(rd(Some(0), None));
    assert!(rd(None, Some(0)));
    assert!(!rd(Some(1), Some(0)), "the machine allows it");
    assert!(rd(Some(0), Some(1)));
    assert_eq!(Vendor::of(Path::new(EDGE)), Some(Vendor::Edge));
    assert_eq!(
        Vendor::of(Path::new(r"C:\x\CHROME.exe")),
        Some(Vendor::Chrome)
    );
    assert_eq!(Vendor::of(Path::new(r"C:\x\chromium.exe")), None);
}

#[test]
fn developer_tools_disallowed_blocks_too_the_machines_value_first() {
    let dev = |machine, user| blocked_by([None, None], [machine, user]);
    assert!(!dev(None, None));
    assert!(dev(Some(2), None));
    assert!(dev(None, Some(2)), "the user's, with none on the machine");
    assert!(
        !dev(Some(1), Some(2)),
        "the machine allows the developer tools"
    );
    assert!(dev(Some(2), Some(1)));
    for allowed in [0, 1, 3] {
        assert!(!dev(Some(allowed), None), "{allowed}");
    }
    assert!(
        blocked_by([Some(1), None], [Some(2), None]),
        "either value blocks on its own"
    );
    assert!(blocked_by([Some(0), None], [Some(1), None]));
}

#[test]
fn each_vendor_has_its_own_policy_key() {
    assert_eq!(POLICY_VALUE, "RemoteDebuggingAllowed");
    assert_eq!(DEVTOOLS_VALUE, "DeveloperToolsAvailability");
    assert_eq!(
        Vendor::Edge.policy_key(),
        r"SOFTWARE\Policies\Microsoft\Edge"
    );
    assert_eq!(
        Vendor::Chrome.policy_key(),
        r"SOFTWARE\Policies\Google\Chrome"
    );
}
