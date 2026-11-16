//! PowerShell's own quoting (`about_Quoting_Rules`) through the whole guard: a matter named
//! with an apostrophe, single-quoted the PowerShell way (`'…\O''Brien & Co Lease\held.txt'`),
//! names its file, and a write to a held file there is refused.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::super::reach::Reach;
use super::super::shell::shell_targets;
use super::super::{Escape, Targets, bash_escape};
use super::{Fixture, deny, guard_json};
use crate::write;

/// Holds the file at `relative` (under the matter `m`, `/` between parts): writes it and Word's
/// owner file beside it (its name's first two characters dropped). Its path.
fn hold(fx: &Fixture, relative: &str) -> PathBuf {
    let path = PathBuf::from(fx.path(&format!("m/{relative}")));
    let folder = path.parent().unwrap();
    write::create_dir_all(folder).unwrap();
    write::write_file(&path, b"x").unwrap();
    let name = path.file_name().unwrap().to_str().unwrap();
    let owner: String = name.chars().skip(2).collect();
    write::write_file(&folder.join(format!("~${owner}")), b"x").unwrap();
    path
}

/// `text` in PowerShell single quotes: each `'` doubled.
fn single(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The files the path layer finds in the PowerShell command `command`, run in the matter.
fn path_layer(fx: &Fixture, command: &str) -> Vec<PathBuf> {
    let matter = fx.matter();
    let reach = Reach::new(&matter);
    match shell_targets(
        Some(&Value::String(command.to_owned())),
        &matter,
        Escape::Backtick,
        &reach,
    ) {
        Ok(Targets::Shell { files, .. }) => files.into_iter().map(|(path, _)| path).collect(),
        _ => Vec::new(),
    }
}

/// Whether `found` holds `path`, compared by its canonical form.
fn finds(found: &[PathBuf], path: &Path) -> bool {
    let want = dunce::canonicalize(path).unwrap();
    found
        .iter()
        .any(|file| dunce::canonicalize(file).is_ok_and(|file| file == want))
}

/// The VM's row (real claude.exe 2.1.293): `Set-Content -LiteralPath` with the path in single
/// quotes, the apostrophe doubled. The path layer finds the file, single-quoted, double-quoted
/// and relative.
#[test]
fn a_matter_named_with_an_apostrophe_single_quoted_is_found_by_path() {
    let fx = Fixture::new("ps-obrien-path");
    for folder in ["O'Brien Lease", "O'Brien & Co Lease"] {
        let held = hold(&fx, &format!("{folder}/held.txt"));
        let full = held.to_str().unwrap();
        for command in [
            format!("Set-Content -LiteralPath {} -Value x", single(full)),
            format!("Set-Content -LiteralPath \"{full}\" -Value x"),
            format!(
                "Set-Content -LiteralPath {} -Value x",
                single(&format!("{folder}/held.txt"))
            ),
        ] {
            assert!(finds(&path_layer(&fx, &command), &held), "{command}");
        }
    }
}

/// A held document whose own name has an apostrophe, single-quoted the PowerShell way: both
/// layers read `O''Brien` as `O'Brien`, so the write is refused.
#[test]
fn a_held_name_with_an_apostrophe_single_quoted_is_refused() {
    let fx = Fixture::new("ps-obrien-name");
    hold(&fx, "O'Brien & Co Lease.docx");
    for command in [
        "Set-Content -LiteralPath 'O''Brien & Co Lease.docx' -Value x",
        "Set-Content -Path 'O''Brien & Co Lease.docx' -Value x",
        "Remove-Item 'O''Brien & Co Lease.docx'",
    ] {
        let input = json!({ "command": command });
        assert_eq!(
            guard_json(&fx.hook("PowerShell", &input)),
            deny("O'Brien & Co Lease.docx"),
            "{command}"
        );
    }
}

/// The matter and file names the matrix holds: punctuation lawyers use, other scripts, a long
/// path, and PowerShell's own special characters.
fn names() -> Vec<String> {
    let long = format!("Long Matter {}", "Annexure ".repeat(17))
        .trim_end()
        .to_owned();
    assert!(long.len() > 160, "the long name");
    [
        "O'Brien & Co Lease",
        "Smith, Jones & Co",
        "A & B Holdings",
        "Sharma v State 2024",
        "M\u{fc}ller GmbH",
        "शर्मा बनाम राज्य",
        long.as_str(),
        "Tick`Tock Ltd",
        "Fees $500 (final)",
        "Exhibit [1]",
        "O\u{2019}Brien \u{201c}Quoted\u{201d}",
    ]
    .iter()
    .map(|name| (*name).to_owned())
    .collect()
}

/// `text` written for PowerShell: in single quotes (each single quote doubled), in double quotes
/// (each double quote doubled, a backtick before `` ` `` and `$`), and bare (a backtick before
/// each blank, quote and character PowerShell would read otherwise).
fn forms(text: &str) -> [String; 3] {
    let single_quote =
        |c: char| matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}');
    let double_quote = |c: char| matches!(c, '"' | '\u{201c}' | '\u{201d}' | '\u{201e}');
    let mut single = String::from("'");
    let mut double = String::from("\"");
    let mut bare = String::new();
    for c in text.chars() {
        if single_quote(c) {
            single.push(c);
        }
        single.push(c);
        if double_quote(c) {
            double.push(c);
        } else if matches!(c, '`' | '$') {
            double.push('`');
        }
        double.push(c);
        if c.is_whitespace() || single_quote(c) || double_quote(c) || "`$&,;|<>(){}@#".contains(c) {
            bare.push('`');
        }
        bare.push(c);
    }
    single.push('\'');
    double.push('"');
    [single, double, bare]
}

/// Each name, held as a document of its own: refused to Claude's `Write` by its path, and to
/// `PowerShell`'s `Set-Content` and `Remove-Item` through `-Path` and `-LiteralPath` with the name
/// single-quoted, double-quoted and bare, relative and absolute.
#[test]
fn a_held_document_with_any_name_is_refused_in_every_quoting() {
    for (at, name) in names().iter().enumerate() {
        let fx = Fixture::new(&format!("ps-names-{at}"));
        let file = format!("{name}.docx");
        let held = hold(&fx, &file);
        assert_eq!(
            guard_json(&fx.hook("Write", &json!({"file_path": held, "content": "x"}))),
            deny(&file),
            "Write {file}"
        );
        let full = held.to_str().unwrap().to_owned();
        for spelled in [file.as_str(), full.as_str()] {
            for form in forms(spelled) {
                for command in [
                    format!("Set-Content -LiteralPath {form} -Value x"),
                    format!("Set-Content -Path {form} -Value x"),
                    format!("Remove-Item {form}"),
                ] {
                    let input = json!({ "command": command });
                    assert_eq!(
                        guard_json(&fx.hook("PowerShell", &input)),
                        deny(&file),
                        "{command}"
                    );
                }
            }
        }
    }
}

/// Each name as the matter's folder, its document held: the path layer finds the file in every
/// quoting, whatever the name backstop does.
#[test]
fn a_folder_with_any_name_is_found_by_path_in_every_quoting() {
    let fx = Fixture::new("ps-folders");
    for name in names() {
        let held = hold(&fx, &format!("{name}/held.txt"));
        for spelled in [
            format!("{name}/held.txt"),
            held.to_str().unwrap().to_owned(),
        ] {
            for form in forms(&spelled) {
                for parameter in ["-Path", "-LiteralPath"] {
                    let command = format!("Set-Content {parameter} {form} -Value x");
                    assert!(finds(&path_layer(&fx, &command), &held), "{command}");
                }
            }
        }
    }
}

/// Square brackets are a wildcard under `-Path` and literal under `-LiteralPath`: a held
/// `Exhibit 1.docx` is refused to `-Path 'Exhibit [1].docx'`, which matches it. Under
/// `-LiteralPath` the name backstop's wildcard rule refuses too, on the safe side.
#[test]
fn brackets_under_path_match_a_held_name() {
    let fx = Fixture::new("ps-brackets");
    hold(&fx, "Exhibit 1.docx");
    for command in [
        "Set-Content -Path 'Exhibit [1].docx' -Value x",
        "Set-Content -Path \"Exhibit [1].docx\" -Value x",
        "Set-Content -LiteralPath 'Exhibit [1].docx' -Value x",
    ] {
        let input = json!({ "command": command });
        assert_eq!(
            guard_json(&fx.hook("PowerShell", &input)),
            deny("Exhibit 1.docx"),
            "{command}"
        );
    }
}

/// An unquoted comma makes an array, each element a file; a quoted comma is part of a name.
#[test]
fn an_array_of_paths_is_checked_element_by_element() {
    let fx = Fixture::new("ps-array");
    for command in [
        "Set-Content -Path 'Free.docx','Contract.docx' -Value x",
        "Set-Content -Path Free.docx,Contract.docx -Value x",
        "Remove-Item -LiteralPath 'Free.docx', \"Contract.docx\"",
    ] {
        let input = json!({ "command": command });
        assert_eq!(
            guard_json(&fx.hook("PowerShell", &input)),
            deny("Contract.docx"),
            "{command}"
        );
    }
    let held = hold(&fx, "Smith, Jones & Co/held.txt");
    let command = "Set-Content -LiteralPath 'Smith, Jones & Co\\held.txt' -Value x";
    assert!(finds(&path_layer(&fx, command), &held), "{command}");
    let input = json!({ "command": command });
    assert_eq!(
        guard_json(&fx.hook("PowerShell", &input)),
        deny("Smith, Jones & Co/held.txt")
    );
    let free =
        json!({ "command": "Set-Content -LiteralPath 'Smith, Jones & Co\\new.txt' -Value x" });
    assert_eq!(guard_json(&fx.hook("PowerShell", &free)), "");
}

/// Codex's hook input as a real Codex 0.162.1 sent it on Windows (VM, 16 Nov 2026): `tool_name`
/// `Bash`, the model's bare PowerShell as `command`, no `--matter` (Codex's hook entry).
fn codex_hook(fx: &Fixture, command: &str) -> serde_json::Value {
    json!({
        "session_id": "01a1e61a-df8c-7c31-911c-298eeaa43757",
        "turn_id": "01a1e61a-e096-7703-9dc5-37cd51ecae2f",
        "transcript_path": "C:\\vm2l\\w1139\\hookcap\\cxhome\\sessions\\rollout.jsonl",
        "cwd": fx.matter().to_str().unwrap(),
        "hook_event_name": "PreToolUse",
        "model": "gpt-6.1-sol",
        "permission_mode": "bypassPermissions",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_use_id": "exec-fce5cf09-e4fa-4a8d-b560-e44efa90677f",
    })
}

/// L8: under Codex's hook on Windows a `Bash` command is PowerShell (Codex runs it with
/// `powershell.exe -Command`), so `'O''Brien & Sons Lease.docx'` names the held document there;
/// it is read the Bash way too, so a POSIX spelling is still refused. On macOS and Linux Codex's
/// shell is Bash, and the reading is unchanged: Bash reads `'O''Brien'` as `OBrien`. Claude
/// Code's `Bash` (with `--matter`) is Bash everywhere.
#[test]
fn codex_bash_is_read_as_powershell_on_windows() {
    assert_eq!(bash_escape(true, true), Escape::Backtick);
    assert_eq!(bash_escape(true, false), Escape::Backslash);
    assert_eq!(bash_escape(false, true), Escape::Backslash);
    let fx = Fixture::new("codex-windows");
    let held = "O'Brien & Sons Lease.docx";
    hold(&fx, held);
    let powershell = codex_hook(
        &fx,
        "Set-Content -LiteralPath 'O''Brien & Sons Lease.docx' -Value 'hello'",
    );
    let posix = codex_hook(&fx, "printf hello > 'O'\\''Brien & Sons Lease.docx'");
    assert_eq!(guard_json(&posix), deny(held));
    let expected = if cfg!(windows) {
        deny(held)
    } else {
        String::new()
    };
    assert_eq!(guard_json(&powershell), expected);
}
