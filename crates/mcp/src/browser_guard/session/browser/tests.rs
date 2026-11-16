//! Windows' fallback over the browsers to try, modelled on any OS: `browsers::choose` over a
//! made-up machine, `plan`, then the loop with a stand-in for each start.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::super::super::PARENT_GONE;
use super::super::super::browsers::{Folders, Unavailable, Vendor, choose};
use super::{StartError, first_to_start, plan};

const EDGE: &str = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
const PER_USER: &str = r"C:\Users\a\AppData\Local\Google\Chrome\Application\chrome.exe";

fn folders() -> Folders {
    Folders {
        x86: Some(PathBuf::from(r"C:\Program Files (x86)")),
        native: Some(PathBuf::from(r"C:\Program Files")),
        local_app_data: Some(PathBuf::from(r"C:\Users\a\AppData\Local")),
        any_host_choice: false,
    }
}

fn fold(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

/// The list `plan` makes for a machine holding `files`, its firm blocking `blocked`, with the
/// host's choice `host`.
fn planned(
    host: &str,
    files: &[&str],
    blocked: &[Vendor],
) -> Result<Vec<String>, super::StartFailure> {
    let files: BTreeSet<String> = files.iter().map(|f| f.to_ascii_lowercase()).collect();
    let choice = choose(
        Path::new(host),
        &folders(),
        |path| files.contains(&fold(path)),
        |vendor| blocked.contains(&vendor),
    );
    plan(choice, None).map(|list| list.iter().map(|path| fold(path)).collect())
}

/// The loop over `list`, each start failing when its path is in `fails`: what started, and
/// each try's place and whether it was told to start fresh.
fn run(list: &[&str], fails: &[&str]) -> (Result<String, String>, Vec<(usize, String)>) {
    let list: Vec<PathBuf> = list.iter().map(PathBuf::from).collect();
    let mut tries = Vec::new();
    let started = first_to_start(&list, |tried, path| {
        tries.push((tried, fold(path)));
        if fails.iter().any(|f| fold(Path::new(f)) == fold(path)) {
            Err(StartError::Other(format!("{} gave no answer", fold(path))))
        } else {
            Ok(fold(path))
        }
    });
    let started = started
        .map(|(value, path)| {
            assert_eq!(value, fold(&path));
            value
        })
        .map_err(|failure| failure.reason.to_string());
    (started, tries)
}

fn low(path: &str) -> String {
    path.to_ascii_lowercase()
}

#[test]
fn a_blocked_edge_starts_chrome() {
    let list = planned(EDGE, &[EDGE, CHROME], &[Vendor::Edge]).unwrap();
    assert_eq!(list, [low(CHROME)]);
    let (started, tries) = run(&[CHROME], &[]);
    assert_eq!(started, Ok(low(CHROME)));
    assert_eq!(tries, [(0, low(CHROME))]);
}

#[test]
fn a_missing_edge_starts_chrome() {
    let list = planned(EDGE, &[CHROME], &[]).unwrap();
    assert_eq!(list, [low(CHROME)]);
}

#[test]
fn an_edge_that_fails_its_start_hands_over_to_chrome_on_a_fresh_profile() {
    let list = planned(EDGE, &[EDGE, CHROME], &[]).unwrap();
    assert_eq!(list, [low(EDGE), low(CHROME)]);
    let (started, tries) = run(&[EDGE, CHROME], &[EDGE]);
    assert_eq!(started, Ok(low(CHROME)));
    // Place 1: `start_browser` removes the profile before it.
    assert_eq!(tries, [(0, low(EDGE)), (1, low(CHROME))]);
}

#[test]
fn every_browser_failing_gives_each_reason_and_the_parents_going_stops_at_once() {
    let (started, _) = run(&[EDGE, CHROME], &[EDGE, CHROME]);
    let reason = started.unwrap_err();
    assert!(reason.starts_with("no browser started: "), "{reason}");
    assert!(reason.contains("msedge.exe gave no answer"), "{reason}");
    assert!(reason.contains("chrome.exe gave no answer"), "{reason}");
    let (started, _) = run(&[CHROME], &[CHROME]);
    assert_eq!(started, Err(format!("{} gave no answer", low(CHROME))));

    let list = [PathBuf::from(EDGE), PathBuf::from(CHROME)];
    let mut tries = 0;
    let gone = first_to_start(&list, |_, _| -> Result<(), StartError> {
        tries += 1;
        Err(PARENT_GONE.to_owned().into())
    });
    assert_eq!(gone.unwrap_err().reason.to_string(), PARENT_GONE);
    assert_eq!(tries, 1, "Chrome is never started once the parent is gone");
}

/// A browser whose sandbox cannot start (`chrome/sandbox.rs`) is refused with the sandbox's own
/// kind, and no other browser is tried after it.
#[test]
fn a_sandbox_that_cannot_start_stops_the_fallback_at_once() {
    let list = [PathBuf::from(EDGE), PathBuf::from(CHROME)];
    let mut tries = 0;
    let refused = first_to_start(&list, |_, _| -> Result<(), StartError> {
        tries += 1;
        Err(StartError::Sandbox(Some(
            "Sandbox cannot access executable",
        )))
    });
    assert_eq!(
        refused.unwrap_err().reason,
        StartError::Sandbox(Some("Sandbox cannot access executable"))
    );
    assert_eq!(
        tries, 1,
        "Chrome is never tried after Edge's sandbox failed"
    );
}

#[test]
fn edge_and_chrome_both_blocked_tell_the_agent_to_ask_it_for_both() {
    let failure = planned(EDGE, &[EDGE, CHROME], &[Vendor::Edge, Vendor::Chrome]).unwrap_err();
    let both = Unavailable::Blocked {
        edge: true,
        chrome: true,
    };
    assert_eq!(failure.reason.to_string(), both.reason());
    assert_eq!(failure.agent_note, Some(both.agent_note()));
    assert!(
        both.agent_note()
            .contains("don't let apps control Microsoft Edge or Google Chrome"),
        "UC-550's advice"
    );
}

#[test]
fn only_the_blocked_browser_is_named() {
    let edge = planned(EDGE, &[EDGE], &[Vendor::Edge]).unwrap_err();
    let note = edge.agent_note.unwrap();
    assert!(note.contains("control Microsoft Edge."), "{note}");
    assert!(!note.contains("Chrome"), "{note}");
    let chrome = planned(EDGE, &[CHROME, PER_USER], &[Vendor::Chrome]).unwrap_err();
    let note = chrome.agent_note.unwrap();
    assert!(note.contains("control Google Chrome."), "{note}");
    assert!(!note.contains("Edge"), "{note}");
}

#[test]
fn a_per_user_chrome_alone_tells_the_agent_why_it_cannot_be_used() {
    let failure = planned(EDGE, &[PER_USER], &[]).unwrap_err();
    assert_eq!(
        failure.agent_note,
        Some(Unavailable::PerUserOnly.agent_note())
    );
    assert!(
        failure.reason.to_string().contains("per-user"),
        "{}",
        failure.reason
    );
    // Even when the host chose it: it lies outside Program Files.
    let failure = planned(PER_USER, &[PER_USER], &[]).unwrap_err();
    assert_eq!(
        failure.agent_note,
        Some(Unavailable::PerUserOnly.agent_note())
    );
    let none = planned(EDGE, &[], &[]).unwrap_err();
    assert_eq!(
        none.agent_note,
        Some(Unavailable::NotInstalled.agent_note())
    );
}

#[test]
fn a_host_choice_outside_program_files_falls_back_to_the_guards_own_search() {
    let elsewhere = r"C:\Users\a\Downloads\chrome.exe";
    let list = planned(elsewhere, &[elsewhere, EDGE], &[]).unwrap();
    assert_eq!(list, [low(EDGE)]);
}
