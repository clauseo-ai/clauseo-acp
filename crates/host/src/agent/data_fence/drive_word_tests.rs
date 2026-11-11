//! #766's Windows CI, on the fake disk under Windows' rules: a script handed whole to `bash -c`
//! or `sh -lc`, or a script's string, holding forward-slash drive paths (`D:/…/matter`). The
//! whole word, taken as a relative path, has a name ending in a drive (`grep -r x D:`), which the
//! name rule refused, so every such request was refused. The data folder's sibling (the matter,
//! an unrelated folder) and the session's own run folder pass; the data folder, in either slash
//! form, is still refused. A folder variable that points at the shared parent (the runner's
//! `TEMP`) gives no spelling of the data folder.

use std::path::{Path, PathBuf};

#[cfg(unix)]
use serde_json::json;

use super::command::embedded_paths;
use super::words::{Spellings, needles};
#[cfg(unix)]
use super::{DataFenceHealth, FenceCheck, candidates, candidates_roots, candidates_words};

/// Session `s1`'s check of `command` from the matter `/t/matter`, the data folder `/t/data`
/// beside it, read with Windows' rules.
#[cfg(unix)]
fn shell(command: &str) -> FenceCheck {
    let raw = json!({ "command": command });
    FenceCheck {
        data: PathBuf::from("/t/data"),
        allowed: vec![PathBuf::from("/t/data/run/s1")],
        cwd: PathBuf::from("/t/matter"),
        home: None,
        paths: candidates(&[], Some(&raw), true),
        roots: candidates_roots(Some(&raw), true),
        words: candidates_words(Some(&raw), true),
        health: DataFenceHealth::default(),
        vars: Vec::new(),
    }
}

/// `\` separates, and `D:/t` is absolute, only on Windows itself: the backslash forms are in
/// [`a_variable_at_the_shared_parent_gives_no_spelling_of_the_sibling`], and a search of the
/// parent in `command_tests.rs`, which runs on the Windows CI.
#[cfg(unix)]
#[test]
fn a_script_with_forward_slash_drive_paths_is_judged_by_where_they_lead() {
    use super::network_tests::refused;
    use crate::network_paths::resolve::tests::{Fake, FakeFs};
    use crate::network_paths::tests::plain_net;

    let fs = FakeFs::with(&[
        ("/t/data/state.sqlite", Fake::File),
        ("/t/data/run/s1/work/out.txt", Fake::File),
        ("/t/data/run/s1/x/run.py", Fake::File),
        ("/t/matter/sub/a.txt", Fake::File),
        ("/t/outside/x", Fake::File),
    ]);
    let net = plain_net(true);
    for command in [
        "bash -c 'grep -r x D:/t/matter'",
        "sh -lc 'ls D:/t/outside && wc -l D:/t/matter/sub/a.txt'",
        "bash -c 'cat D:/t/data/run/s1/work/out.txt'",
        r#"python3 -c "exec(open('D:\t\data/run/s1/x/run.py').read())""#,
        r#"python3 -c "print(open('D:/t/matter/sub/a.txt').read())""#,
    ] {
        assert!(!refused(&shell(command), &net, &fs), "{command}");
    }
    for command in [
        "bash -c 'cat D:/t/data/state.sqlite'",
        "sh -lc 'ls D:/t/outside && cat D:/t/data/state.sqlite'",
        "bash -c 'cat D:/t/data/run/s2/work/out.txt'",
        r#"python3 -c "print(open('D:/t/data/state.sqlite').read())""#,
    ] {
        assert!(refused(&shell(command), &net, &fs), "{command}");
    }
}

/// Every folder variable at the data folder's parent (the runner's `TEMP` is `D:\a\_temp`, the
/// data folder `D:\a\_temp\t\data`): the spellings are the data folder's alone, so a word naming
/// its sibling matches none, and one naming it matches in both slash forms.
#[test]
fn a_variable_at_the_shared_parent_gives_no_spelling_of_the_sibling() {
    let data = Path::new(r"D:\a\_temp\t\data");
    let var = |_: &str| Some(PathBuf::from(r"D:\a\_temp"));
    let needles = needles(&Spellings {
        paths: [data, data],
        short: None,
        home: None,
        var: &var,
        windows: true,
    });
    let found = |word: &str| embedded_paths(word, &needles, data, true, true);
    for word in [
        "grep -r x D:/a/_temp/t/matter",
        r"type D:\a\_temp\t\matter\sub\a.txt",
        "ls D:/a/_temp/t/outside",
    ] {
        assert_eq!(found(word), Vec::<PathBuf>::new(), "{word}");
    }
    for word in [
        "cat D:/a/_temp/t/data/state.sqlite",
        r"type D:\a\_temp\t\data\state.sqlite",
        r"type %APPDATA%\t\data\state.sqlite",
    ] {
        assert!(!found(word).is_empty(), "{word}");
    }
}
