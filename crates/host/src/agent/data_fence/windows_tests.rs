//! The fence under Windows' rules, on the fake disk so it runs in every test run: a `..` after a
//! junction is dropped by its text (the bypass through a deep junction is refused), and the data
//! folder's other spellings inside a shell word (its short name, its folder variables, its home
//! forms, doubled separators) are found.

use std::path::{Path, PathBuf};

use super::command::embedded_paths;
#[cfg(unix)]
use super::network_tests::{fence, refused};
use super::words::{Spellings, needles};
#[cfg(unix)]
use crate::network_paths::resolve::tests::{Fake, FakeFs};
#[cfg(unix)]
use crate::network_paths::tests::plain_net;

/// A junction `j` to a deep folder, then `j\..\..\AppData\Roaming\Clauseo\x`: Windows opens
/// the data folder (the `..` pairs go by text first), so the fence refuses it; Unix would back
/// up from the junction's target, elsewhere.
#[cfg(unix)]
#[test]
fn a_dot_dot_back_out_of_a_deep_junction_into_the_data_folder_is_refused() {
    let names = [
        ("/u/m/j", Fake::Link("/deep/a/b/c".into())),
        ("/deep/a/b/c", Fake::Dir),
        ("/u/AppData/Roaming/Clauseo/x", Fake::File),
    ];
    let mut check = fence("/u/m", &["/u/m/j/../../AppData/Roaming/Clauseo/x"]);
    check.data = PathBuf::from("/u/AppData/Roaming/Clauseo");
    let fs = FakeFs::with(&names);
    assert!(refused(&check, &plain_net(true), &fs));
    assert!(!fs.touched().iter().any(|path| path.starts_with("/deep")));
    let unix = FakeFs::with(&names);
    assert!(!refused(&check, &plain_net(false), &unix));
    // A plain `..` after a junction is the folder beside it, checked as any other path.
    let fs = FakeFs::with(&names);
    assert!(!refused(
        &fence("/u/m", &["/u/m/j/../x"]),
        &plain_net(true),
        &fs
    ));
}

/// The data folder `C:\Users\a\AppData\Roaming\Clauseo`, with its short name, under `APPDATA`.
fn roaming(short: bool) -> Vec<String> {
    let data = Path::new(r"C:\Users\a\AppData\Roaming\Clauseo");
    let var = |name: &str| match name {
        "APPDATA" => Some(PathBuf::from(r"C:\Users\a\AppData\Roaming")),
        "LOCALAPPDATA" => Some(PathBuf::from(r"C:\Users\a\AppData\Local")),
        _ => None,
    };
    needles(&Spellings {
        paths: [data, data],
        short: short.then(|| PathBuf::from(r"C:\Users\JONATH~1\AppData\Roaming\Clauseo")),
        home: Some(Path::new(r"C:\Users\a")),
        var: &var,
        windows: true,
    })
}

/// `path` with `/` read as `\` and each run of separators made one.
fn one_separator(path: &Path) -> PathBuf {
    let mut out = String::new();
    for c in path.to_string_lossy().chars() {
        let c = if c == '/' { '\\' } else { c };
        if !(c == '\\' && out.ends_with('\\')) {
            out.push(c);
        }
    }
    PathBuf::from(out)
}

fn found(word: &str, needles: &[String], target: &str) -> Vec<PathBuf> {
    embedded_paths(word, needles, Path::new(target), true, true)
}

/// Each spelling of the data folder inside a word is checked as the data folder's path.
#[test]
fn every_alias_of_the_data_folder_inside_a_shell_word_is_found() {
    let data = r"C:\Users\a\AppData\Roaming\Clauseo";
    let needles = roaming(true);
    let want = PathBuf::from(format!(r"{data}\state.sqlite"));
    for word in [
        r"C:\Users\JONATH~1\AppData\Roaming\Clauseo\state.sqlite",
        r"%APPDATA%\Clauseo\state.sqlite",
        r"%appdata%/clauseo/state.sqlite",
        r"$env:APPDATA\Clauseo\state.sqlite",
        r"${env:APPDATA}\Clauseo\state.sqlite",
        r"$APPDATA/Clauseo\state.sqlite",
        r"${APPDATA}\Clauseo\state.sqlite",
        r"~\AppData\Roaming\Clauseo\state.sqlite",
        r"~/AppData/Roaming/Clauseo/state.sqlite",
        r"$HOME\AppData\Roaming\Clauseo\state.sqlite",
        r"$env:USERPROFILE\AppData\Roaming\Clauseo\state.sqlite",
        r"%USERPROFILE%\AppData\Roaming\Clauseo\state.sqlite",
        r"C:\\Users\\a\\AppData\\Roaming\\Clauseo\\state.sqlite",
        r"C://Users//a//AppData//Roaming//Clauseo//state.sqlite",
        r"C:\Users/a\\AppData//Roaming\Clauseo\state.sqlite",
        r"c:\users\A\APPDATA\roaming\CLAUSEO\state.sqlite",
    ] {
        let got = found(&format!("type \"{word}\""), &needles, data);
        assert!(!got.is_empty(), "{word}");
        // The rest of the word keeps its separators; the path it names is the same.
        assert!(
            got.iter().any(|path| one_separator(path) == want),
            "{word}: {got:?}"
        );
    }
    // Without its short name the short spelling is not searched; another folder never matches.
    let plain = roaming(false);
    assert_eq!(
        found(r"C:\Users\JONATH~1\AppData\Roaming\Clauseo\x", &plain, data),
        Vec::<PathBuf>::new()
    );
    assert_eq!(
        found(r"%APPDATA%\Other\x", &needles, data),
        Vec::<PathBuf>::new()
    );
    assert_eq!(
        found(r"%LOCALAPPDATA%\Clauseo\x", &needles, data),
        Vec::<PathBuf>::new()
    );
}

/// A data folder under the local app data is searched for by `LOCALAPPDATA`'s forms.
#[test]
fn a_data_folder_under_local_app_data_is_found_by_its_variable() {
    let data = Path::new(r"C:\Users\a\AppData\Local\Clauseo");
    let var =
        |name: &str| (name == "LOCALAPPDATA").then(|| PathBuf::from(r"C:\Users\a\AppData\Local"));
    let needles = needles(&Spellings {
        paths: [data, data],
        short: None,
        home: None,
        var: &var,
        windows: true,
    });
    for word in [
        r"%LOCALAPPDATA%\Clauseo\logs",
        r"$env:LOCALAPPDATA\Clauseo\logs",
        r"${env:LOCALAPPDATA}\Clauseo\logs",
        r"$LOCALAPPDATA/Clauseo/logs",
    ] {
        assert_eq!(
            found(word, &needles, r"C:\Users\a\AppData\Local\Clauseo").len(),
            1,
            "{word}"
        );
    }
}

/// End to end: a shell word with a doubled separator in the data folder's home form is
/// refused.
#[cfg(unix)]
#[test]
fn a_doubled_separator_in_a_shell_word_is_refused() {
    let fs = FakeFs::with(&[("/home/a/.clauseo/state.sqlite", Fake::File)]);
    let mut check = fence("/m", &[]);
    check.data = PathBuf::from("/home/a/.clauseo");
    check.home = Some(PathBuf::from("/home/a"));
    for word in [
        "/home//a//.clauseo//state.sqlite",
        "$HOME//.clauseo/state.sqlite",
        "~/.clauseo///state.sqlite",
    ] {
        check.words = vec![format!("cat {word}")];
        assert!(refused(&check, &plain_net(false), &fs), "{word}");
    }
    check.words = vec!["cat /home//a//.other/x".to_owned()];
    assert!(!refused(&check, &plain_net(false), &fs));
}

/// A word that mixes long and short names (`C:\Users\JONATH~1\AppData\…`,
/// `C:\Users\Jonathan\APPDAT~1\…`) is found: each name of the data folder is searched in its long
/// and its short form, and so is the rest under the home folder.
#[test]
fn a_word_mixing_long_and_short_names_of_the_data_folder_is_found() {
    let data = r"C:\Users\Jonathan\AppData\Roaming\Clauseo";
    let var = |name: &str| {
        (name == "APPDATA").then(|| PathBuf::from(r"C:\Users\Jonathan\AppData\Roaming"))
    };
    let needles = needles(&Spellings {
        paths: [Path::new(data), Path::new(data)],
        short: Some(PathBuf::from(r"C:\Users\JONATH~1\APPDAT~1\Roaming\Clauseo")),
        home: Some(Path::new(r"C:\Users\Jonathan")),
        var: &var,
        windows: true,
    });
    let want = PathBuf::from(format!(r"{data}\state.sqlite"));
    for word in [
        r"C:\Users\JONATH~1\AppData\Roaming\Clauseo\state.sqlite",
        r"C:\Users\Jonathan\APPDAT~1\Roaming\Clauseo\state.sqlite",
        r"C:\Users\JONATH~1\APPDAT~1\Roaming\Clauseo\state.sqlite",
        r"c:/users/jonath~1/appdata/roaming/clauseo/state.sqlite",
        r"%USERPROFILE%\APPDAT~1\Roaming\Clauseo\state.sqlite",
        r"~\APPDAT~1\Roaming\Clauseo\state.sqlite",
    ] {
        let got = found(&format!("type \"{word}\""), &needles, data);
        assert!(
            got.iter().any(|path| one_separator(path) == want),
            "{word}: {got:?}"
        );
    }
    // Another short name in the same place is another folder.
    assert_eq!(
        found(
            r"C:\Users\JONATH~2\AppData\Roaming\Clauseo\x",
            &needles,
            data
        ),
        Vec::<PathBuf>::new()
    );
}

/// Names that don't pair up one for one, or too many mixes, still search the short spelling.
#[test]
fn an_unpaired_or_huge_short_name_still_searches_the_all_short_spelling() {
    let none = |_: &str| None;
    let search = |data: &str, short: &str| {
        needles(&Spellings {
            paths: [Path::new(data), Path::new(data)],
            short: Some(PathBuf::from(short)),
            home: None,
            var: &none,
            windows: true,
        })
    };
    let unpaired = search(r"C:\Long Name\Clauseo", r"C:\LONGNA~1\x\Clauseo");
    assert!(unpaired.iter().any(|n| n == r"C:\LONGNA~1\x\Clauseo"));
    let long = r"C:\Aaaaaaaaa\Bbbbbbbbb\Ccccccccc\Ddddddddd\Eeeeeeeee\Clauseo";
    let short = r"C:\AAAAAA~1\BBBBBB~1\CCCCCC~1\DDDDDD~1\EEEEEE~1\Clauseo";
    let huge = search(long, short);
    assert!(huge.iter().any(|n| n == short));
    assert!(huge.iter().any(|n| n == long));
    assert!(huge.len() <= 2, "{huge:?}");
    // Four names with a short form: every mix (sixteen).
    let four = search(
        r"C:\Aaaaaaaaa\Bbbbbbbbb\Ccccccccc\Ddddddddd\Clauseo",
        r"C:\AAAAAA~1\BBBBBB~1\CCCCCC~1\DDDDDD~1\Clauseo",
    );
    assert_eq!(four.len(), 16, "{four:?}");
}
