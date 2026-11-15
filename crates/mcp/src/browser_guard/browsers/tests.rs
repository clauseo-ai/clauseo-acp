//! The Windows browser order and its two reasons for none, modelled on any OS.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{Choice, POLICY_VALUE, ProgramFiles, Unavailable, Vendor, blocked_by, choose};

const EDGE_X86: &str = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
const EDGE: &str = r"C:\Program Files\Microsoft\Edge\Application\msedge.exe";
const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
const CHROME_X86: &str = r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe";

fn dirs() -> ProgramFiles {
    ProgramFiles {
        x86: Some(PathBuf::from(r"C:\Program Files (x86)")),
        native: Some(PathBuf::from(r"C:\Program Files")),
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
    assert_eq!(blocked.unavailable(), Some(Unavailable::Blocked));
    let only_edge_blocked = on(EDGE_X86, &[EDGE_X86], &[Vendor::Edge]);
    assert_eq!(only_edge_blocked.unavailable(), Some(Unavailable::Blocked));
    for reason in [Unavailable::NotInstalled, Unavailable::Blocked] {
        assert!(reason.agent_note().contains("Tell the lawyer"));
        assert!(reason.reason().starts_with("no browser to start"));
    }
}

#[test]
fn chrome_for_testing_and_a_per_user_chrome_are_only_tried_as_the_hosts_choice() {
    let cft = r"C:\Users\a\AppData\Roaming\Clauseo\browsers\chrome-157\chrome-win64\chrome.exe";
    let user = r"C:\Users\a\AppData\Local\Google\Chrome\Application\chrome.exe";
    let choice = on(EDGE_X86, &[cft, user, EDGE_X86], &[]);
    assert_eq!(
        found(&choice.try_in_order),
        paths(&[EDGE_X86]),
        "never found by the guard itself"
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
    assert!(!blocked_by(None, None));
    assert!(blocked_by(Some(0), None));
    assert!(blocked_by(None, Some(0)));
    assert!(!blocked_by(Some(1), Some(0)), "the machine allows it");
    assert!(blocked_by(Some(0), Some(1)));
    assert_eq!(Vendor::of(Path::new(EDGE)), Some(Vendor::Edge));
    assert_eq!(
        Vendor::of(Path::new(r"C:\x\CHROME.exe")),
        Some(Vendor::Chrome)
    );
    assert_eq!(Vendor::of(Path::new(r"C:\x\chromium.exe")), None);
}

#[test]
fn each_vendor_has_its_own_policy_key() {
    assert_eq!(POLICY_VALUE, "RemoteDebuggingAllowed");
    assert_eq!(
        Vendor::Edge.policy_key(),
        r"SOFTWARE\Policies\Microsoft\Edge"
    );
    assert_eq!(
        Vendor::Chrome.policy_key(),
        r"SOFTWARE\Policies\Google\Chrome"
    );
}
