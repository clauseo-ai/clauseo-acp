//! The seam's `Debug` prints kinds and flags, never a path, host or matter name.

use std::path::PathBuf;

use super::{Dialog, Pick};

/// Words that must never appear in a `Debug` line: each path, host and name below.
const SECRET: [&str; 6] = ["Users", "rohan", "Brown", "Smith", "court", "brief"];

fn assert_clean(debug: &str) {
    for word in SECRET {
        assert!(!debug.contains(word), "{word} in {debug}");
    }
}

#[test]
fn debug_prints_no_path_host_or_name() {
    let path = PathBuf::from("/Users/rohan/Matters/Brown/brief.docx");
    let dialogs = [
        Dialog::Folder {
            matter_name: "Smith".to_owned(),
            path: path.clone(),
            is_file: true,
            other_matter: Some("Brown".to_owned()),
        },
        Dialog::ConfirmPick {
            matter_name: "Smith".to_owned(),
            path,
            is_file: false,
            other_matter: None,
        },
        Dialog::Site {
            host: "court.example".to_owned(),
            shared_hosting: true,
        },
        Dialog::SwitchOff {
            matter_name: "Smith".to_owned(),
        },
    ];
    let lines: Vec<String> = dialogs.iter().map(|d| format!("{d:?} {d:#?}")).collect();
    for line in &lines {
        assert_clean(line);
    }
    assert!(lines[0].starts_with("Folder { is_file: true, other_matter: true, .. }"));
    assert!(lines[1].starts_with("ConfirmPick { is_file: false, other_matter: false, .. }"));
    assert!(lines[2].starts_with("Site { shared_hosting: true, .. }"));
    assert!(lines[3].starts_with("SwitchOff { .. }"));
    let pick = Pick {
        start: PathBuf::from("/Users/rohan/Matters/Brown"),
        files_too: true,
        matter_name: "Smith".to_owned(),
    };
    let line = format!("{pick:?} {pick:#?}");
    assert_clean(&line);
    assert!(line.starts_with("Pick { files_too: true, .. }"));
}

/// The fake's `Debug` holds what it was asked and its picks: counts only.
#[cfg(feature = "test-support")]
#[test]
fn the_fakes_debug_prints_no_path_either() {
    use super::{DialogAnswer, FakeDialogs, NativeDialogs};
    let fake = FakeDialogs::new([DialogAnswer::AllowOnce])
        .with_picks([Some(PathBuf::from("/Users/rohan/Matters/Brown"))]);
    fake.ask(Dialog::SwitchOff {
        matter_name: "Smith".to_owned(),
    });
    assert_clean(&format!("{fake:?} {fake:#?}"));
}
