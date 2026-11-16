//! A Claude launch on macOS and Linux carries its sandbox or is refused, never neither
//! (`ClaudeSandbox::for_launch`, `Off::refusal`); Windows carries none by design.

use std::path::PathBuf;

use clauseo_protocol::ErrorCode;
use serde_json::{Value, json};

use super::sandbox::{ClaudeSandbox, Off, SandboxOs, WILDCARDS, Wild, wild_folder};
use super::sandbox_tests::{Session, settings};

/// What a launch carries: the flag settings' `sandbox` object, or the refusal's code.
#[derive(Debug, PartialEq)]
enum Launch {
    Sandboxed(Value),
    Refused(ErrorCode),
}

/// The launch of `session` on `os`, through the session's flag settings. Panics on a macOS or
/// Linux launch with neither a sandbox nor a refusal.
fn launch(session: &Session, os: SandboxOs) -> Launch {
    match ClaudeSandbox::for_launch(os, &session.paths()) {
        Ok(Some(sandbox)) => {
            let settings = settings(session, Some(&sandbox), false, false, &json!({}));
            Launch::Sandboxed(settings["sandbox"].clone())
        }
        Ok(None) => panic!("a {os:?} launch with neither a sandbox nor a refusal"),
        Err(error) => Launch::Refused(error.code()),
    }
}

fn texts(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

/// An ordinary macOS launch: the sandbox is on, no command breaks out of it, a sandbox that
/// can't start fails Claude rather than run it unsandboxed, and the matter, work, tmp and notes
/// folders are writable.
#[test]
fn an_ordinary_mac_launch_carries_the_sandbox() {
    let session = Session::new("s-1");
    let Launch::Sandboxed(sandbox) = launch(&session, SandboxOs::Mac) else {
        panic!("refused");
    };
    assert_eq!(sandbox["enabled"], true);
    assert_eq!(sandbox["allowUnsandboxedCommands"], false);
    assert_eq!(sandbox["failIfUnavailable"], true);
    let write = texts(&sandbox["filesystem"]["allowWrite"]);
    for folder in [&session.matter, &session.work, &session.tmp, &session.notes] {
        assert!(
            write.contains(&folder.to_str().unwrap()),
            "{folder:?}: {write:?}"
        );
    }
}

/// What a case's launch must be.
type Expected<'a> = dyn Fn(&Launch) -> bool + 'a;

/// Stage C's real names: a forgotten matter's folder and a bracketed one.
// unix-paths-ok: sandbox paths as text, never opened
const STAR: &str = "/Users/rohan/Documents/stagec-1097/Test*";
// unix-paths-ok: sandbox paths as text, never opened
const BRACKETS: &str = "/Users/rohan/Documents/stagec-proof-claude-[x]";

/// The matrix: each input's launch on macOS and on Linux is the sandbox or the refusal it
/// should be, and never neither.
#[test]
fn every_mac_and_linux_launch_has_a_sandbox_or_a_refusal() {
    let sandboxed = |_: &Launch| true;
    let refused = |code| move |launch: &Launch| *launch == Launch::Refused(code);
    let own = refused(ErrorCode::MatterFolderWildcard);
    let clauseos = refused(ErrorCode::AgentSandboxWildcard);
    let unavailable = refused(ErrorCode::AgentSandboxUnavailable);
    // Escaped and kept today; refused once `[` joins the wildcards.
    let brackets_refused = WILDCARDS.contains(&'[');
    let brackets = |launch: &Launch| brackets_refused == matches!(launch, Launch::Refused(_));
    let cases: Vec<(&str, Session, &Expected<'_>)> = vec![
        ("ordinary", Session::new("s-1"), &sandboxed),
        ("own *", with_matter(STAR), &own),
        (
            "own ?",
            // unix-paths-ok: sandbox paths as text, never opened
            with_matter("/Users/rohan/Documents/stagec-1097/Test?"),
            &own,
        ),
        // unix-paths-ok: sandbox paths as text, never opened
        ("parent *", with_matter("/Users/l/Clients*/A"), &own),
        ("brackets", with_matter(BRACKETS), &brackets),
        // Another matter's `*` stays a pattern; refused only when it would cover this one.
        ("other matter *", with_other(STAR), &sandboxed),
        (
            "other matter over this",
            // unix-paths-ok: sandbox paths as text, never opened
            with_other("/Users/l/Clients/?"),
            &clauseos,
        ),
        (
            "notes ?",
            // unix-paths-ok: sandbox paths as text, never opened
            with_notes("/Users/l/Library/Clauseo?/agent-state/m-1/notes"),
            &clauseos,
        ),
        ("no temp", no_temp(), &unavailable),
    ];
    for (name, session, expected) in &cases {
        for os in [SandboxOs::Mac, SandboxOs::Linux] {
            let launch = launch(session, os);
            assert!(expected(&launch), "{name} on {os:?}: {launch:?}");
            if let Launch::Sandboxed(sandbox) = &launch {
                assert_eq!(sandbox["enabled"], true, "{name} on {os:?}");
            }
        }
    }
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let mut session = Session::new("s-1");
        session
            .other_matters
            .push(PathBuf::from(OsStr::from_bytes(b"/Users/l/\xff")));
        let launch = launch(&session, SandboxOs::Linux);
        assert_eq!(launch, Launch::Refused(ErrorCode::AgentSandboxUnavailable));
    }
}

/// A neighbour named with `*` or `?` in the matter's parent (`/Users/l/Clients/What?` beside
/// `/Users/l/Clients/A`): on Linux its deny widens to the parent, which holds this matter, so the
/// launch is refused with UC-471 (`agent.sandbox_wildcard`), never run without the sandbox; on
/// macOS the pattern covers neither, so the launch carries it. #1085's own test states the same
/// for its branch.
#[test]
fn a_wildcard_neighbour_in_the_parent_refuses_on_linux_and_never_turns_the_sandbox_off() {
    // unix-paths-ok: sandbox paths as text, never opened
    let session = with_other("/Users/l/Clients/What?");
    assert_eq!(
        launch(&session, SandboxOs::Linux),
        Launch::Refused(ErrorCode::AgentSandboxWildcard)
    );
    assert_eq!(
        ClaudeSandbox::for_os(SandboxOs::Linux, &session.paths()),
        Err(Off::CoversOwn)
    );
    let Launch::Sandboxed(mac) = launch(&session, SandboxOs::Mac) else {
        panic!("macOS refused a neighbour whose pattern covers nothing of this matter's");
    };
    let denied = mac["filesystem"]["denyRead"].as_array().unwrap();
    // unix-paths-ok: sandbox paths as text, never opened
    assert!(denied.iter().any(|path| path == "/Users/l/Clients/What?"));
}

/// Windows carries no sandbox and no refusal: none was meant to be there.
#[test]
fn a_windows_launch_carries_no_sandbox_and_is_not_refused() {
    let session = Session::new("s-1");
    let carried = ClaudeSandbox::for_launch(SandboxOs::Windows, &session.paths());
    assert!(matches!(carried, Ok(None)), "{carried:?}");
}

/// The wildcard's folder is the deepest name holding one: the folder to rename.
#[test]
fn the_wildcard_folder_is_the_deepest_name_holding_one() {
    assert_eq!(wild_folder(STAR), "Test*");
    // unix-paths-ok: sandbox paths as text, never opened
    assert_eq!(wild_folder("/Users/l/Clients*/A"), "Clients*");
    assert_eq!(wild_folder("/a?/b*/c"), "b*");
    let mut session = Session::new("s-1");
    session.matter = PathBuf::from(STAR);
    let off = ClaudeSandbox::for_os(SandboxOs::Mac, &session.paths()).unwrap_err();
    let folder = "Test*".to_owned();
    assert_eq!(
        off,
        Off::Wildcard(Wild {
            folder,
            in_matter: true
        })
    );
    assert_eq!(Off::Windows.refusal().map(|error| error.code()), None);
}

fn with_matter(path: &str) -> Session {
    let mut session = Session::new("s-1");
    session.matter = PathBuf::from(path);
    session
}

fn with_other(path: &str) -> Session {
    let mut session = Session::new("s-1");
    session.other_matters.push(PathBuf::from(path));
    session
}

fn with_notes(path: &str) -> Session {
    let mut session = Session::new("s-1");
    session.notes = PathBuf::from(path);
    session
}

fn no_temp() -> Session {
    let mut session = Session::new("s-1");
    session.claude_tmp = None;
    session
}
