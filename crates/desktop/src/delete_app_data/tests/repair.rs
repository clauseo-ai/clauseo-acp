//! Repair mode (`/CLAUSEO_REPAIR`, hooks.nsh), which the desktop app's `repair_install` (#777)
//! passes to this version's per-user installer before it exits: the installer waits, bounded, for
//! Clauseo's process and main window to go, before the template's app-running check and the admin
//! step's; it starts Clauseo once as the user at the end of an install that ran to its end, never
//! after an abort; and it skips the finish page. The per-machine installer ignores the switch.

use std::collections::HashMap;

use super::admin_copy::{ASK, admin_code, assert_run, code_of, macro_body};
use super::nsis_run::{Machine, Node, defines, parse_all, preprocess};
use super::sources::{hook_code, line_with};
use super::template::TEMPLATE;

const WAIT: &str = "CLAUSEO_REPAIR_WAIT";
const RELAUNCH: &str = "CLAUSEO_REPAIR_RELAUNCH";
const RUN_AS_USER: &str = "nsis_tauri_utils::RunAsUser \"$INSTDIR\\${MAINBINARYNAME}.exe\" \"\"";

/// `defines` with the template's `INSTALLMODE` set to `mode`.
fn in_mode(mode: &str) -> HashMap<String, String> {
    let mut defines = defines(&hook_code());
    defines.insert("INSTALLMODE".to_owned(), mode.to_owned());
    defines.insert("NSIS_PTR_SIZE".to_owned(), "4".to_owned());
    defines
}

/// The code lines of `Section Install` in the template, up to its `SectionEnd`.
fn section_install(template: &[String]) -> &[String] {
    let start = line_with(template, &["Section Install"]);
    let end = start + line_with(&template[start..], &["SectionEnd"]);
    &template[start..end]
}

/// How many lines of `hooks.nsh` and `admin_copy.nsh`'s code hold `part`.
fn count(part: &str) -> usize {
    let mut code = hook_code();
    code.extend(admin_code());
    code.iter().filter(|line| line.contains(part)).count()
}

#[test]
fn the_repair_switch_is_parsed_like_the_others_and_only_by_the_per_user_installer() {
    let code = hook_code();
    line_with(
        &code,
        &["!define CLAUSEO_REPAIR_SWITCH \"/CLAUSEO_REPAIR\""],
    );
    line_with(&code, &["Var ClauseoRepair"]);
    let pre = &macro_body(&code, "NSIS_HOOK_PREINSTALL")[1..];
    // Per user: 0, then the switch read as the template reads /P, and the wait only under it.
    let user = preprocess(pre, &in_mode("currentUser"));
    assert_eq!(
        user,
        [
            "StrCpy $ClauseoRepair 0",
            "Push $R0",
            "${GetOptions} $CMDLINE \"${CLAUSEO_REPAIR_SWITCH}\" $R0",
            "${IfNot} ${Errors}",
            "StrCpy $ClauseoRepair 1",
            "${EndIf}",
            "Pop $R0",
            "${If} $ClauseoRepair = 1",
            &format!("!insertmacro {WAIT}"),
            "${EndIf}",
        ]
    );
    // Per machine: the switch is never read, so nothing of repair mode runs there.
    let machine = preprocess(pre, &in_mode("perMachine"));
    assert_eq!(machine, ["StrCpy $ClauseoRepair 0"]);
    let post = &macro_body(&code, "NSIS_HOOK_POSTINSTALL")[1..];
    let machine = preprocess(post, &in_mode("perMachine"));
    assert!(
        !machine.iter().any(|line| line.contains("REPAIR")),
        "{machine:?}"
    );
    let user = preprocess(post, &in_mode("currentUser"));
    let relaunches = user
        .iter()
        .filter(|line| **line == format!("!insertmacro {RELAUNCH}"))
        .count();
    assert_eq!(relaunches, 1, "{user:?}");
    // The wait and the relaunch are inserted nowhere else.
    assert_eq!(count(&format!("!insertmacro {WAIT}")), 1);
    assert_eq!(count(&format!("!insertmacro {RELAUNCH}")), 1);
}

#[test]
fn the_repair_wait_comes_before_the_templates_check_and_the_admin_steps() {
    let template = code_of(TEMPLATE);
    let install = section_install(&template);
    // The template inserts the pre-install hook right before its own CheckIfAppIsRunning.
    assert_run(
        install,
        &[
            "!ifmacrodef NSIS_HOOK_PREINSTALL",
            "!insertmacro NSIS_HOOK_PREINSTALL",
            "!endif",
            "!insertmacro CheckIfAppIsRunning \"$INSTDIR\\${MAINBINARYNAME}.exe\" \"${PRODUCTNAME}\"",
        ],
    );
    // The admin step's own check (adminCopyAppRunning, exit 24) is in the post-install hook,
    // which comes after.
    let pre = line_with(install, &["!insertmacro NSIS_HOOK_PREINSTALL"]);
    let post = line_with(install, &["!insertmacro NSIS_HOOK_POSTINSTALL"]);
    assert!(pre < post);
    let code = hook_code();
    let body = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    line_with(body, &[&format!("!insertmacro {ASK}")]);
    let admin = admin_code();
    let ask = macro_body(&admin, ASK);
    line_with(ask, &["FindWindow $R5 \"${CLAUSEO_MAIN_WINDOW_CLASS}\""]);
    line_with(ask, &["SetErrorLevel ${CLAUSEO_EXIT_APP_RUNNING}"]);
    // The wait itself: the template's own test for the file (Restart Manager), and the window.
    let wait = macro_body(&code, WAIT);
    for part in [
        "!insertmacro RestartManager_StartSession $R0",
        "!insertmacro RestartManager_RegisterFile $R0 \"$INSTDIR\\${MAINBINARYNAME}.exe\"",
        "System::Call 'RSTRTMGR::RmGetList(p R0, *i .r1, *i .r2, p 0, *i .r3) i .r0'",
        "${If} $0 = ${ERROR_MORE_DATA}",
        "FindWindow $1 \"${CLAUSEO_MAIN_WINDOW_CLASS}\"",
        "!insertmacro RestartManager_EndSession $R0",
    ] {
        line_with(wait, &[part]);
    }
}

/// The wait run as written, with Restart Manager's answer `rm` (its `RmGetList` result, 234 for
/// a process using the file) and `FindWindow`'s `window` the same at every look: how many times it
/// slept. It must leave the registers and the stack as it found them and end its session.
fn sleeps(rm: u32, window: u32) -> usize {
    let code = hook_code();
    let mut defines = defines(&code);
    defines.insert("ERROR_MORE_DATA".to_owned(), "234".to_owned());
    let body: Vec<String> = macro_body(&code, WAIT)[1..]
        .iter()
        .map(|line| {
            if line.starts_with("!insertmacro RestartManager_StartSession") {
                "StrCpy $R0 7".to_owned()
            } else if line.starts_with("!insertmacro RestartManager_RegisterFile") {
                "StrCpy $0 0".to_owned()
            } else if line.starts_with("!insertmacro RestartManager_EndSession") {
                "DetailPrint ended".to_owned()
            } else if line.contains("RmGetList") {
                format!("StrCpy $0 {rm}")
            } else if line.starts_with("FindWindow $1 ") {
                format!("StrCpy $1 {window}")
            } else if let Some(ms) = line.strip_prefix("Sleep ") {
                format!("DetailPrint slept:{ms}")
            } else {
                line.clone()
            }
        })
        .collect();
    let mut machine = Machine::new(defines, None);
    for register in ["$0", "$1", "$2", "$3", "$R0", "$R1", "$R2", "$R3"] {
        machine.set_var(register, "kept");
    }
    machine.run(&parse_all(&body));
    for register in ["$0", "$1", "$2", "$3", "$R0", "$R1", "$R2", "$R3"] {
        assert_eq!(machine.vars[register], "kept", "{register}");
    }
    assert!(machine.stack.is_empty(), "{:?}", machine.stack);
    assert_eq!(machine.said.last().unwrap(), "DetailPrint ended");
    let slept: Vec<&String> = machine
        .said
        .iter()
        .filter(|said| said.starts_with("DetailPrint slept"))
        .collect();
    assert!(
        slept.iter().all(|said| *said == "DetailPrint slept:250"),
        "{slept:?}"
    );
    slept.len()
}

#[test]
fn the_repair_wait_waits_for_both_and_at_most_ten_seconds_every_250_ms() {
    let code = hook_code();
    line_with(&code, &["!define CLAUSEO_REPAIR_WAIT_MS 10000"]);
    line_with(&code, &["!define CLAUSEO_REPAIR_POLL_MS 250"]);
    // Both gone at the first look: no wait.
    assert_eq!(sleeps(0, 0), 0);
    // Either one still there, or both: 40 looks of 250 ms, 10 s, then it carries on.
    assert_eq!(sleeps(234, 0), 40);
    assert_eq!(sleeps(0, 1), 40);
    assert_eq!(sleeps(234, 1), 40);
}

#[test]
fn the_repair_relaunch_starts_clauseo_once_as_the_user_after_the_admin_step() {
    let code = hook_code();
    let body = &macro_body(&code, RELAUNCH)[1..];
    assert_eq!(
        body,
        [
            "${If} $ClauseoRepair = 1",
            "${AndIf} $ClauseoAdminTimedOut <> 1",
            RUN_AS_USER,
            "StrCpy $PassiveMode 1",
            "SetAutoClose true",
            "${EndIf}",
        ]
    );
    // As the template's RunMainBinary starts it (the finish page's "Run Clauseo").
    let template = code_of(TEMPLATE);
    assert_run(
        &template,
        &["Function RunMainBinary", RUN_AS_USER, "FunctionEnd"],
    );
    // The hooks start Clauseo here and nowhere else.
    assert_eq!(count("RunAsUser"), 1);
    // After the ask, whatever its outcome (the ask has no Abort or Quit), and outside any Push
    // and Pop: RunAsUser may leave its result on the stack.
    let post = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    let relaunch = line_with(post, &[&format!("!insertmacro {RELAUNCH}")]);
    assert_eq!(post[relaunch - 1], format!("!insertmacro {ASK}"));
    let admin = admin_code();
    let ask = macro_body(&admin, ASK);
    assert!(
        !ask.iter()
            .any(|line| line.starts_with("Abort") || line == "Quit"),
        "{ask:?}"
    );
    // A `System::Call` with `?e` pushes GetLastError's value, which a later `Pop` takes: it
    // counts as a Push here.
    let depth = post[..relaunch].iter().fold(0i32, |depth, line| {
        let pushes =
            line.starts_with("Push ") || (line.starts_with("System::Call ") && line.contains("?e"));
        depth + i32::from(pushes) - i32::from(line.starts_with("Pop "))
    });
    assert_eq!(depth, 0, "{post:?}");
}

#[test]
fn the_repair_relaunch_never_runs_after_an_abort() {
    // The relaunch is the post-install hook's, which the template inserts at the end of Section
    // Install: after its app-running check (whose cancel aborts), the files and everything else
    // that can abort; an abort ends the section before it. Only the passive close follows.
    let template = code_of(TEMPLATE);
    let install = section_install(&template);
    let post = line_with(install, &["!insertmacro NSIS_HOOK_POSTINSTALL"]);
    assert!(line_with(install, &["!insertmacro CheckIfAppIsRunning"]) < post);
    assert!(line_with(install, &["File \"${MAINBINARYSRCPATH}\""]) < post);
    assert!(line_with(install, &["WriteUninstaller"]) < post);
    let tail: Vec<&str> = install[post + 1..].iter().map(String::as_str).collect();
    assert_eq!(
        tail,
        [
            "!endif",
            "${If} $PassiveMode = 1",
            "SetAutoClose true",
            "${EndIf}"
        ]
    );
    // No hook runs on a failed or cancelled install, so nothing else can start Clauseo then.
    let code = hook_code();
    for callback in [".onInstFailed", ".onUserAbort", ".onGUIEnd"] {
        assert!(
            !code.iter().any(|line| line.contains(callback)),
            "hooks.nsh defines {callback}"
        );
    }
    // The relaunch is inserted only in the post-install hook.
    let body = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    line_with(body, &[&format!("!insertmacro {RELAUNCH}")]);
}

#[test]
fn repair_mode_skips_the_finish_page_and_closes_the_installer() {
    // RELAUNCH sets $PassiveMode 1 and SetAutoClose true (the test above); the template's finish
    // page then skips itself through SkipIfPassive, which aborts the page under $PassiveMode 1.
    let template = code_of(TEMPLATE);
    let finish = line_with(&template, &["!insertmacro MUI_PAGE_FINISH"]);
    assert_eq!(
        template[finish - 1],
        "!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive"
    );
    assert_run(
        &template,
        &[
            "Function SkipIfPassive",
            "${IfThen} $PassiveMode = 1  ${|} Abort ${|}",
            "FunctionEnd",
        ],
    );
    // .onInstSuccess starts Clauseo under $PassiveMode only with /R, which the app never passes:
    // the relaunch stays the one start.
    let success = line_with(&template, &["Function .onInstSuccess"]);
    let end = success + line_with(&template[success..], &["FunctionEnd"]);
    let body = &template[success..end];
    let run = line_with(body, &["nsis_tauri_utils::RunAsUser"]);
    assert!(line_with(body, &["${GetOptions} $CMDLINE \"/R\" $R0"]) < run);
    // The relaunch's effect on the passive path, run as written: $PassiveMode 1 in repair mode
    // only, and not after a wait for the elevated instance that timed out.
    let code = hook_code();
    let lines = &macro_body(&code, RELAUNCH)[1..];
    for (repair, timed_out, skipped) in [("1", "", true), ("0", "", false), ("1", "1", false)] {
        let passive = run_relaunch(lines, repair, timed_out);
        assert_eq!(passive, skipped, "repair {repair}, timed out {timed_out:?}");
    }
}

/// RELAUNCH's chain with `$ClauseoRepair` and `$ClauseoAdminTimedOut` as given (renamed to
/// registers, which the interpreter reads): whether it set `$PassiveMode` 1.
fn run_relaunch(lines: &[String], repair: &str, timed_out: &str) -> bool {
    let lines: Vec<String> = lines
        .iter()
        .map(|line| {
            line.replace("$ClauseoRepair", "$R1")
                .replace("$ClauseoAdminTimedOut", "$R2")
                .replace("StrCpy $PassiveMode 1", "DetailPrint passive")
                .replace(RUN_AS_USER, "DetailPrint run")
                .replace("SetAutoClose true", "DetailPrint close")
        })
        .collect();
    let nodes = parse_all(&lines);
    assert!(matches!(nodes.as_slice(), [Node::If(..)]));
    let mut machine = Machine::new(HashMap::new(), None);
    machine.set_var("$R1", repair);
    machine.set_var("$R2", timed_out);
    machine.run(&nodes);
    let said: Vec<&str> = machine.said.iter().map(String::as_str).collect();
    match said.as_slice() {
        [
            "DetailPrint run",
            "DetailPrint passive",
            "DetailPrint close",
        ] => true,
        [] => false,
        other => panic!("{other:?}"),
    }
}
