//! When Windows security removed or blocked `Clauseo.exe` as setup wrote it, the installer must
//! not say it finished: `CLAUSEO_APP_EXE_CHECK` (`hooks.nsh`), first in `NSIS_HOOK_POSTINSTALL`
//! in both installers, shows `clauseoAppRemovedByAv` (no box under `/S`), sets exit code 28 and
//! aborts, so there is no finish page and no "Run Clauseo". The macro's lines are read from the
//! source and run by `nsis_run` on a fake machine.

use std::collections::HashMap;

use super::admin_copy::macro_body;
use super::nsis_run::{Machine, defines, parse_all, preprocess};
use super::sources::{hook_code, read};

const CHECK: &str = "CLAUSEO_APP_EXE_CHECK";
const INSTDIR: &str = r"C:\Users\a\AppData\Local\Clauseo";
const EXE: &str = r"C:\Users\a\AppData\Local\Clauseo\Clauseo.exe";

/// The defines the macro sees: `hooks.nsh`'s, and the template's `MAINBINARYNAME` for Clauseo.
fn check_defines() -> HashMap<String, String> {
    let mut defines = defines(&hook_code());
    defines.insert("MAINBINARYNAME".to_owned(), "Clauseo".to_owned());
    defines
}

/// The check run with `Clauseo.exe` in `$INSTDIR` or not, silent (`/S`) or not, followed by a
/// line standing for the rest of the hook: the machine it leaves.
fn run_check(exe_there: bool, silent: bool) -> Machine {
    let defines = check_defines();
    let check = preprocess(&macro_body(&hook_code(), CHECK)[1..], &defines);
    let mut machine = Machine::new(defines, None);
    machine.silent = silent;
    machine.set_var("$INSTDIR", INSTDIR);
    if exe_there {
        machine.files.push(EXE.to_owned());
    }
    machine.inline.insert(CHECK.to_owned(), parse_all(&check));
    let hook = [
        format!("!insertmacro {CHECK}"),
        "DetailPrint rest-of-the-hook".to_owned(),
    ];
    machine.run(&parse_all(&hook));
    assert!(machine.stack.is_empty(), "{:?}", machine.stack);
    machine
}

#[test]
fn the_check_is_the_first_thing_the_post_install_hook_does_in_both_installers() {
    let code = hook_code();
    let post = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    // Outside the mode block, before the language value and the admin steps.
    assert_eq!(post[1], format!("!insertmacro {CHECK}"), "{post:?}");
    let inserts = code
        .iter()
        .filter(|line| **line == format!("!insertmacro {CHECK}"))
        .count();
    assert_eq!(inserts, 1, "{code:?}");
    // The box in the file's install style (safe_temp.nsh, uninstall_in_place.nsh), brought to
    // the front, with /SD so /S shows none; then 28 and Abort. No mode test: /P and /UPDATE get
    // the box too, since the lawyer is there and nothing else says why Clauseo won't open.
    assert_eq!(
        macro_body(&code, CHECK)[1..],
        [
            "${IfNot} ${FileExists} \"$INSTDIR\\${MAINBINARYNAME}.exe\"",
            "DetailPrint \"$(clauseoAppRemovedByAv)\"",
            "MessageBox MB_OK|MB_ICONSTOP|MB_SETFOREGROUND|MB_TOPMOST \"$(clauseoAppRemovedByAv)\" \
             /SD IDOK",
            "SetErrorLevel ${CLAUSEO_EXIT_APP_EXE_REMOVED}",
            "Abort",
            "${EndIf}",
        ]
    );
}

#[test]
fn with_clauseo_exe_there_the_check_says_nothing_and_the_hook_goes_on() {
    for silent in [false, true] {
        let machine = run_check(true, silent);
        assert!(!machine.aborted, "silent {silent}");
        assert_eq!(
            machine.said,
            ["DetailPrint rest-of-the-hook"],
            "silent {silent}"
        );
    }
}

#[test]
fn with_clauseo_exe_gone_an_interactive_or_passive_install_shows_the_box_sets_28_and_aborts() {
    let machine = run_check(false, false);
    assert!(machine.aborted);
    assert_eq!(
        machine.said,
        [
            "DetailPrint $(clauseoAppRemovedByAv)",
            "MessageBox $(clauseoAppRemovedByAv)",
            "SetErrorLevel 28",
            "Abort",
        ]
    );
}

#[test]
fn with_clauseo_exe_gone_a_silent_install_shows_no_box_sets_28_and_aborts() {
    let machine = run_check(false, true);
    assert!(machine.aborted);
    assert_eq!(
        machine.said,
        [
            "DetailPrint $(clauseoAppRemovedByAv)",
            "SetErrorLevel 28",
            "Abort",
        ]
    );
}

#[test]
fn exit_code_28_is_free_and_listed_for_firm_it() {
    let code = hook_code();
    let line = |name: &str| {
        code.iter()
            .find_map(|line| line.strip_prefix(&format!("!define {name} ")))
            .unwrap_or_else(|| panic!("no {name}"))
            .parse::<u32>()
            .unwrap()
    };
    let removed = line("CLAUSEO_EXIT_APP_EXE_REMOVED");
    assert_eq!(removed, 28);
    // NSIS's own (0, 1, 2), and what Intune and SCCM read as success, a reboot or a retry; the
    // others' distinctness is admin_copy_ask.rs's the_installers_exit_codes_are_their_own.
    assert!(![0, 1, 2, 1223, 1618, 1641, 1707, 3010].contains(&removed));
    // Both install tables in section 1 (/S and interactive), and section 10's.
    let firm_it = read("../../docs/firm-it.md");
    let row = "| 28 | Install: nothing usable installed, because Windows Security (or another \
               antivirus) removed or blocked `Clauseo.exe` while it was being written; allow \
               Clauseo in Windows Security's Protection history, then install again |";
    assert_eq!(firm_it.lines().filter(|line| *line == row).count(), 3);
}

#[test]
fn the_words_are_the_approved_english_and_a_hindi_that_keeps_the_names() {
    let text = |file: &str, constant: &str| {
        read(file)
            .lines()
            .find_map(|line| {
                line.strip_prefix(&format!("LangString clauseoAppRemovedByAv {constant} "))
            })
            .unwrap_or_else(|| panic!("{file}: no clauseoAppRemovedByAv"))
            .to_owned()
    };
    assert_eq!(
        text("windows/nsis/English.nsh", "${LANG_ENGLISH}"),
        "\"Windows Security stopped part of Clauseo while it was installing, so Clauseo can't \
         open. Open Windows Security, choose Protection history, allow Clauseo, then install \
         Clauseo again.\""
    );
    let hindi = text("windows/nsis/Hindi.nsh", "${LANG_HINDI}");
    for kept in ["Windows Security", "Clauseo", "(Protection history)"] {
        assert!(hindi.contains(kept), "{kept}: {hindi}");
    }
    assert!(!hindi.contains('$') && !hindi.contains('\\'), "{hindi}");
}
