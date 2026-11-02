use std::path::PathBuf;

use super::{Escape, Folders, command_words, moved, script};

/// The folders after each of `segments` (words split at spaces), from `/m`, which does not
/// exist, so resolving through symlinks changes nothing.
fn walk(segments: &[&str], escape: Escape) -> Folders {
    let mut folders = Folders {
        reached: vec![PathBuf::from("/m")],
        unknown: false,
        pushed: 0,
    };
    for segment in segments {
        let words: Vec<String> = segment.split(' ').map(str::to_owned).collect();
        moved(&mut folders, &words, escape);
    }
    folders
}

/// Reached `paths`, in order, and whether the folder is unknown.
fn reached(paths: &[&str], unknown: bool) -> Folders {
    Folders {
        reached: paths.iter().map(PathBuf::from).collect(),
        unknown,
        pushed: 0,
    }
}

fn without_pushes(mut folders: Folders) -> Folders {
    folders.pushed = 0;
    folders
}

#[cfg(unix)]
#[test]
fn a_cd_adds_each_folder_it_may_reach() {
    let posix = Escape::Backslash;
    let w = |segments: &[&str]| without_pushes(walk(segments, posix));
    assert_eq!(w(&["cd D"]), reached(&["/m", "/m/D"], false));
    assert_eq!(w(&["cd D", "cd .."]), reached(&["/m", "/m/D", "/"], false));
    assert_eq!(w(&["cd ./D/../E/"]), reached(&["/m", "/m/E"], false));
    assert_eq!(w(&["cd -P D"]), reached(&["/m", "/m/D"], false));
    assert_eq!(w(&["(cd D)"]), reached(&["/m", "/m/D"], false));
    assert_eq!(w(&["{ cd D; }"]), reached(&["/m", "/m/D"], false));
    assert_eq!(w(&["cd /x/y"]), reached(&["/m", "/x/y"], false));
    for prefix in [
        "builtin", "command", "!", "then", "do", "else", "time", "CDPATH=", "A=b",
    ] {
        assert_eq!(
            w(&[&format!("{prefix} cd D")]),
            reached(&["/m", "/m/D"], false),
            "{prefix}"
        );
    }
    assert_eq!(w(&["echo cd D"]), reached(&["/m"], false));
    for unknown in [
        "cd",
        "cd ~",
        "cd ~/x",
        "cd -",
        "cd $(pwd)/D",
        "pushd +1",
        "pushd -1",
    ] {
        assert!(w(&[unknown]).unknown, "{unknown}");
    }
    // Unknown stays set, and an absolute folder is still added.
    assert_eq!(
        w(&["cd $(pwd)", "cd /m/sub"]),
        reached(&["/m", "/m/sub"], true)
    );
    assert_eq!(
        w(&["pushd D", "cd E", "popd"]),
        reached(&["/m", "/m/D", "/m/E", "/m/D/E"], false)
    );
    assert!(w(&["popd"]).unknown);
    // Too many folders: unknown.
    let many: Vec<String> = (0..5).map(|i| format!("cd d{i}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(w(&many).unknown);
}

#[cfg(unix)]
#[test]
fn powershell_set_location_adds_its_folder() {
    let ps = Escape::Backtick;
    let w = |segments: &[&str]| without_pushes(walk(segments, ps));
    assert_eq!(w(&["Set-Location D"]), reached(&["/m", "/m/D"], false));
    assert_eq!(
        w(&["set-location -Path D\\E"]),
        reached(&["/m", "/m/D/E"], false)
    );
    assert_eq!(
        w(&["Set-Location -LiteralPath:'D'"]),
        reached(&["/m", "/m/D"], false)
    );
    assert_eq!(w(&["sl ..\\x"]), reached(&["/m", "/x"], false));
    assert_eq!(w(&["{ cd D }"]), reached(&["/m", "/m/D"], false));
    assert_eq!(
        w(&["Push-Location D", "Pop-Location"]),
        reached(&["/m", "/m/D"], false)
    );
}

#[test]
fn prefixes_are_skipped_before_the_command_name() {
    let words = |text: &str| -> Vec<String> { text.split(' ').map(str::to_owned).collect() };
    assert_eq!(command_words(&words("! A=1 B= time cd x")), words("cd x"));
    assert_eq!(command_words(&words("echo A=1")), words("echo A=1"));
    assert_eq!(command_words(&words("{ (")), Vec::<String>::new());
}

#[test]
fn scripts_handed_to_another_shell_are_found() {
    let words = |items: &[&str]| -> Vec<String> { items.iter().map(|w| (*w).to_owned()).collect() };
    let posix = Escape::Backslash;
    assert_eq!(
        script(&words(&["sh", "-c", "cd D && x"]), posix),
        Some(("cd D && x".to_owned(), Escape::Backslash))
    );
    assert_eq!(
        script(&words(&["/bin/bash", "-lc", "cd D"]), posix),
        Some(("cd D".to_owned(), Escape::Backslash))
    );
    assert_eq!(
        script(&words(&["eval", "cd", "D"]), posix),
        Some(("cd D".to_owned(), Escape::Backslash))
    );
    assert_eq!(
        script(
            &words(&["pwsh.exe", "-NoProfile", "-Command", "cd", "D"]),
            posix
        ),
        Some(("cd D".to_owned(), Escape::Backtick))
    );
    assert_eq!(
        script(&words(&["powershell", "-c", "Set-Location D"]), posix),
        Some(("Set-Location D".to_owned(), Escape::Backtick))
    );
    assert_eq!(script(&words(&["bash", "script.sh"]), posix), None);
    assert_eq!(script(&words(&["bash", "--norc", "x"]), posix), None);
    assert_eq!(script(&words(&["python", "-c", "x"]), posix), None);
}
