//! The installer's side of `--adopt` against the set-up program's own (S15 tier B, PR 1 against
//! PR 2): the arguments `admin_elevated.nsh` and `admin_copy.nsh` pass parse as the arguments
//! `args::parse` takes, the record value names the installer reads are `record_values.rs`'s,
//! and its `CLAUSEO_ADOPT_*` codes are `SetupExit`'s.

use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_sandbox::windows::setup::SetupExit;
use clauseo_sandbox::windows::setup::args::{AdoptUser, Mode, parse};
use clauseo_sandbox::windows::setup::record_values;
use clauseo_sandbox::windows::sids::Sid;

use super::admin_copy::admin_code;
use super::sources::line_with;

const SID: &str = "S-1-5-21-1-2-3-1001";
const INSTALL: &str = r"C:\Users\lawyer\AppData\Local\Clauseo";
const DATA: &str = r"C:\Users\lawyer\AppData\Local\chat.clauseo.app";

/// The `ARGS` of the one `!insertmacro CLAUSEO_ADMIN_COPY '...'` line that `pick` accepts.
fn adopt_args(code: &[String], pick: impl Fn(&str) -> bool) -> String {
    let lines: Vec<&String> = code
        .iter()
        .filter(|line| line.starts_with("!insertmacro CLAUSEO_ADMIN_COPY '") && pick(line))
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    lines[0]
        .strip_prefix("!insertmacro CLAUSEO_ADMIN_COPY '")
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap_or_else(|| panic!("{}", lines[0]))
        .to_owned()
}

/// The program's arguments for the command line `--adopt ARGS` (the `ExecWait` line), its NSIS
/// variables given `values`, split as Windows splits a command line with no escaped quote.
fn command_line(args: &str, values: &[(&str, &str)]) -> Vec<String> {
    let mut line = format!("--adopt {args}");
    for (variable, value) in values {
        line = line.replace(variable, value);
    }
    assert!(!line.contains('$'), "a variable left: {line}");
    let mut words = Vec::new();
    let (mut word, mut quoted, mut any) = (String::new(), false, false);
    for c in line.chars() {
        match c {
            '"' => (quoted, any) = (!quoted, true),
            ' ' if !quoted => {
                if any {
                    words.push(std::mem::take(&mut word));
                }
                any = false;
            }
            c => (word, any) = (word + &c.to_string(), true),
        }
    }
    assert!(!quoted, "{line}");
    if any {
        words.push(word);
    }
    words
}

fn parsed(words: &[String]) -> Mode {
    parse(words.iter().map(OsString::from)).unwrap_or_else(|usage| panic!("{usage}: {words:?}"))
}

#[test]
fn the_installers_adopt_arguments_are_the_ones_the_set_up_program_parses() {
    let code = admin_code();
    // The copy is started with `--adopt ARGS`, nothing before ARGS.
    line_with(
        &code,
        &["ExecWait '\"${CLAUSEO_ADMIN_DIR}\\clauseo-sandbox-setup.exe\" --adopt ${ARGS}' $R8"],
    );

    // Per user (the elevated branch): the SID in $R2, the data folder in $R3.
    let user = adopt_args(&code, |line| line.contains("--owner-sid"));
    let words = command_line(&user, &[("$R2", SID), ("$INSTDIR", INSTALL), ("$R3", DATA)]);
    assert_eq!(
        words,
        [
            "--adopt",
            "--owner-sid",
            SID,
            "--user-install-dir",
            INSTALL,
            "--data-dir",
            DATA
        ]
    );
    assert_eq!(
        parsed(&words),
        Mode::Adopt {
            user_install_dir: PathBuf::from(INSTALL),
            user: Some(AdoptUser {
                owner_sid: Sid::parse(SID).unwrap(),
                data_dir: PathBuf::from(DATA),
            }),
        }
    );

    // Per machine.
    let machine = adopt_args(&code, |line| line.contains("--per-machine"));
    let install = r"C:\Program Files\Clauseo";
    let words = command_line(&machine, &[("$INSTDIR", install)]);
    assert_eq!(
        words,
        ["--adopt", "--per-machine", "--user-install-dir", install]
    );
    assert_eq!(
        parsed(&words),
        Mode::Adopt {
            user_install_dir: PathBuf::from(install),
            user: None,
        }
    );
}

#[test]
fn the_record_values_the_installer_reads_are_the_set_up_programs() {
    let code = admin_code();
    for (define, value) in [
        ("CLAUSEO_RECORD_COPY_OWNER", record_values::ADMIN_COPY_OWNER),
        ("CLAUSEO_RECORD_MACHINE", record_values::MACHINE_OWNER),
        ("CLAUSEO_RECORD_OWNER_SID", record_values::OWNER_SID),
        ("CLAUSEO_RECORD_VERSION", record_values::ADMIN_COPY_VERSION),
        ("CLAUSEO_RECORD_KEY", r"SOFTWARE\Clauseo\Sandbox"),
    ] {
        line_with(&code, &[&format!("!define {define} \"{value}\"")]);
    }
}

#[test]
fn the_installers_adopt_codes_are_setup_exits() {
    let code = admin_code();
    let defined: Vec<(String, u8)> = code
        .iter()
        .filter_map(|line| line.strip_prefix("!define CLAUSEO_ADOPT_"))
        .map(|rest| {
            let (name, value) = rest.split_once(' ').unwrap();
            (name.to_owned(), value.parse().unwrap())
        })
        .collect();
    let expected = [
        ("DONE", SetupExit::Done),
        ("USAGE", SetupExit::Usage),
        ("OTHER_ACCOUNT", SetupExit::OtherOwner),
        ("FAILED", SetupExit::Failed),
        ("LOCK_HELD", SetupExit::LockHeld),
        ("DATA_DIR", SetupExit::DataDirNotOwned),
        ("NOT_ELEVATED", SetupExit::NotElevated),
        ("NOT_ADMIN_COPY", SetupExit::NotAdminCopy),
        ("DOWNGRADE", SetupExit::Downgrade),
    ];
    let wanted: Vec<(String, u8)> = expected
        .iter()
        .map(|(name, exit)| ((*name).to_owned(), exit.as_code()))
        .collect();
    assert_eq!(defined, wanted);
    let codes: Vec<u8> = wanted.iter().map(|(_, code)| *code).collect();
    assert_eq!(codes, [0, 1, 2, 3, 5, 6, 7, 8, 9]);
    // Every SetupExit but the removal's own RemovedPartly (4) is one --adopt may give, so a code
    // added to SetupExit fails here until the installer names it.
    let mut all: Vec<u8> = SetupExit::ALL
        .iter()
        .filter(|exit| **exit != SetupExit::RemovedPartly)
        .map(|exit| exit.as_code())
        .collect();
    all.sort_unstable();
    assert_eq!(all, codes);
}
