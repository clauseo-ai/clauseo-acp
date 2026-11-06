//! The setup exe's command line: every mode, every bad form, and the quoting round trip.

use super::*;

fn parsed(line: &[&str]) -> Result<Mode, Usage> {
    parse(line.iter().map(OsString::from))
}

#[test]
fn each_mode_parses_in_full() {
    assert_eq!(
        parsed(&[
            "--set-up",
            "--parent",
            "4242",
            "--data-dir",
            r"C:\Users\Rohan\AppData\x"
        ]),
        Ok(Mode::SetUp {
            parent: 4242,
            data_dir: PathBuf::from(r"C:\Users\Rohan\AppData\x"),
            add: 0,
        })
    );
    // The flags in any order, and an extend.
    assert_eq!(
        parsed(&[
            "--set-up",
            "--add",
            "4",
            "--data-dir",
            "d:/data",
            "--parent",
            "7"
        ]),
        Ok(Mode::SetUp {
            parent: 7,
            data_dir: PathBuf::from("d:/data"),
            add: 4,
        })
    );
    assert_eq!(
        parsed(&[
            "--set-up",
            "--parent",
            "7",
            "--data-dir",
            r"\\?\C:\data",
            "--add",
            "32"
        ]),
        Ok(Mode::SetUp {
            parent: 7,
            data_dir: PathBuf::from(r"\\?\C:\data"),
            add: 32,
        })
    );
    assert_eq!(
        parsed(&["--reclaim", "--parent", "9"]),
        Ok(Mode::Reclaim { parent: 9 })
    );
    assert_eq!(parsed(&["--remove"]), Ok(Mode::Remove));
}

#[test]
fn every_bad_form_is_a_usage_error() {
    let bad: &[&[&str]] = &[
        &[],
        &["--setup"],
        &["set-up"],
        &["--parent", "1"],
        // --set-up missing what it needs.
        &["--set-up"],
        &["--set-up", "--parent", "1"],
        &["--set-up", "--data-dir", r"C:\d"],
        // A flag with no value, twice, unknown, or joined to its value.
        &["--set-up", "--parent", "1", "--data-dir"],
        &[
            "--set-up",
            "--parent",
            "1",
            "--parent",
            "1",
            "--data-dir",
            r"C:\d",
        ],
        &[
            "--set-up",
            "--parent",
            "1",
            "--data-dir",
            r"C:\d",
            "--owner",
            "S-1-5-18",
        ],
        &["--set-up", "--parent=1", "--data-dir", r"C:\d"],
        // --add out of range or not a number.
        &[
            "--set-up",
            "--parent",
            "1",
            "--data-dir",
            r"C:\d",
            "--add",
            "0",
        ],
        &[
            "--set-up",
            "--parent",
            "1",
            "--data-dir",
            r"C:\d",
            "--add",
            "33",
        ],
        &[
            "--set-up",
            "--parent",
            "1",
            "--data-dir",
            r"C:\d",
            "--add",
            "+4",
        ],
        &[
            "--set-up",
            "--parent",
            "1",
            "--data-dir",
            r"C:\d",
            "--add",
            "four",
        ],
        // --reclaim and --remove take nothing else.
        &["--reclaim"],
        &["--reclaim", "--parent", "1", "--data-dir", r"C:\d"],
        &["--reclaim", "--parent", "1", "--add", "1"],
        &["--remove", "--parent", "1"],
        &["--remove", "--data-dir", r"C:\d"],
        &["--remove", "--add", "1"],
        &["--remove", "--per-machine"],
    ];
    for line in bad {
        assert!(parsed(line).is_err(), "{line:?} parsed");
    }
}

#[test]
fn a_parent_that_isnt_a_process_id_is_refused() {
    for parent in ["", "0", "-1", "+5", "12a", "0x10", " 5", "4294967296"] {
        let line = ["--set-up", "--parent", parent, "--data-dir", r"C:\d"];
        let error = parsed(&line).unwrap_err();
        assert!(error.0.contains("--parent"), "{parent:?}: {error}");
    }
}

#[test]
fn a_data_dir_that_isnt_a_full_drive_path_is_refused() {
    for dir in [
        "data",
        r"..\data",
        r"\data",
        "C:",
        "C:data",
        r"\\server\share\data",
        "",
    ] {
        let line = ["--set-up", "--parent", "1", "--data-dir", dir];
        let error = parsed(&line).unwrap_err();
        assert!(error.0.contains("--data-dir"), "{dir:?}: {error}");
    }
}

#[test]
fn the_usage_line_names_every_mode() {
    let text = parsed(&[]).unwrap_err().to_string();
    for mode in ["--set-up", "--reclaim", "--remove"] {
        assert!(text.contains(mode), "{text}");
    }
}

/// The C runtime's split of a command line's arguments (after the program's name), as
/// `CommandLineToArgvW` and Rust's `args` do it, for the round trip.
fn split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
        if chars.peek().is_none() {
            return out;
        }
        let (mut arg, mut quoted) = (String::new(), false);
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    let mut slashes = 1;
                    while chars.next_if_eq(&'\\').is_some() {
                        slashes += 1;
                    }
                    if chars.peek() == Some(&'"') {
                        arg.extend(std::iter::repeat_n('\\', slashes / 2));
                        if slashes % 2 == 1 {
                            arg.push('"');
                            chars.next();
                        }
                    } else {
                        arg.extend(std::iter::repeat_n('\\', slashes));
                    }
                }
                '"' if quoted && chars.peek() == Some(&'"') => {
                    arg.push('"');
                    chars.next();
                }
                '"' => quoted = !quoted,
                ' ' | '\t' if !quoted => break,
                other => arg.push(other),
            }
        }
        out.push(arg);
    }
}

#[test]
fn a_quoted_argument_reads_back_exactly() {
    let cases = [
        r"C:\Users\Rohan\AppData\Roaming\chat.clauseo",
        r"C:\Users\Rohan Shiralkar\AppData\Local\Clauseo",
        r"C:\Program Files\x\",
        r"C:\a\\",
        r#"C:\odd "quoted" name\"#,
        r#"\\""#,
        "",
        "tab\there",
        "--parent",
    ];
    for case in cases {
        let line = format!("--data-dir {} --add 4", quote(case));
        assert_eq!(split(&line), ["--data-dir", case, "--add", "4"], "{line}");
    }
    assert_eq!(quote(r"C:\plain\path"), r"C:\plain\path");
    assert_eq!(quote(r"C:\with space\"), r#""C:\with space\\""#);
}
