use std::path::Path;

use clauseo_protocol::{AgentKind, ErrorCode};

use super::{REFUSES_BACKSLASH, has_backslash, refuse_on};

/// Rohan's matter folder, as Finder showed it: its name holds literal backslashes.
// unix-paths-ok: only its text is read (`has_backslash`); no path API opens it
const ROHANS: &str = r"/Users/rohan/Documents/Clauseo\ Test\ Matter";

#[test]
fn a_backslash_in_a_unix_path_refuses_claude_as_folder_backslash() {
    for folder in [ROHANS, r"/tmp/x\y", r"/tmp/a\ b", r"/tmp/x\y/inside"] {
        let refused =
            refuse_on(Some(AgentKind::Claude), Path::new(folder), false).expect_err(folder);
        assert_eq!(refused.code(), ErrorCode::MatterFolderBackslash, "{folder}");
        assert!(!refused.to_string().contains("/tmp"), "{refused}");
        assert!(!refused.to_string().contains("rohan"), "{refused}");
    }
}

#[test]
fn the_same_names_without_a_backslash_start_claude() {
    for folder in [
        // unix-paths-ok: only its text is read (`has_backslash`); no path API opens it
        "/Users/rohan/Documents/Clauseo Test Matter",
        "/tmp/a b",
        "/tmp/xy",
    ] {
        refuse_on(Some(AgentKind::Claude), Path::new(folder), false).expect(folder);
    }
}

#[test]
fn windows_paths_are_never_refused_for_their_separator() {
    for folder in [r"C:\Users\rohan\Documents\Matter", r"\\server\share\Matter"] {
        refuse_on(Some(AgentKind::Claude), Path::new(folder), true).expect(folder);
        assert!(!has_backslash(Path::new(folder), true), "{folder}");
    }
}

#[test]
fn only_the_listed_kinds_are_refused() {
    // Claude is listed; an agent with no kind (`fake-agent`) never is.
    assert!(REFUSES_BACKSLASH.contains(&AgentKind::Claude));
    for kind in [Some(AgentKind::Claude), Some(AgentKind::Codex), None] {
        let listed = kind.is_some_and(|kind| REFUSES_BACKSLASH.contains(&kind));
        let answer = refuse_on(kind, Path::new(ROHANS), false);
        assert_eq!(answer.is_err(), listed, "{kind:?}");
    }
}
