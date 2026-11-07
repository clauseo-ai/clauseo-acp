//! What the sources must keep when Windows security removed the sandbox's set-up program
//! (`clauseo-sandbox-setup.exe`): both installers check for the copy launches run after installing
//! (the admin copy once the admin section ran, else the one in `$INSTDIR`) and say so, the
//! uninstaller says so when it finds its admin copy (the one its removal starts) gone after a
//! set-up, each with a box or, under `/S`, an exit code of its own that `docs/firm-it.md` lists.

use super::admin_copy::{ASK, admin_code, assert_run, macro_body};
use super::sources::{hook_code, line_with, preuninstall, read};

const SETUP_EXE: &str = "\"$INSTDIR\\clauseo-sandbox-setup.exe\"";
/// The admin copy in Program Files, the one the uninstaller's removal starts (`hooks.nsh`).
const ADMIN_EXE: &str = "\"${CLAUSEO_ADMIN_EXE}\"";
/// The same copy as `admin_copy.nsh` names it, which the installers check after the admin section.
const ADMIN_COPY: &str = "\"${CLAUSEO_ADMIN_DIR}\\clauseo-sandbox-setup.exe\"";

/// The post-install macro's code lines.
fn postinstall(code: &[String]) -> &[String] {
    let start = line_with(code, &["!macro NSIS_HOOK_POSTINSTALL"]);
    let end = start + line_with(&code[start..], &["!macroend"]);
    &code[start..end]
}

/// The English text of `key` in `English.nsh`, quotes included.
fn english(key: &str) -> String {
    read("windows/nsis/English.nsh")
        .lines()
        .find_map(|line| line.strip_prefix(&format!("LangString {key} ${{LANG_ENGLISH}} ")))
        .unwrap_or_else(|| panic!("no LangString {key}"))
        .to_owned()
}

/// The value `hooks.nsh` defines for `name`.
fn defined(code: &[String], name: &str) -> u32 {
    let line = &code[line_with(code, &[&format!("!define {name} ")])];
    line.rsplit(' ').next().unwrap().parse().unwrap()
}

#[test]
fn both_installers_check_for_the_set_up_program_last_and_keep_the_install() {
    let code = hook_code();
    let body = postinstall(&code);
    // After the per-machine block's !endif, so the per-user installer runs it too, and after the
    // admin section, which both installers end that block with.
    let check = body.iter().rposition(|line| line == "Push $R7").unwrap();
    let last_endif = body.iter().rposition(|line| line == "!endif").unwrap();
    assert_eq!(last_endif + 1, check, "{body:?}");
    let expected = [
        "Push $R7".to_owned(),
        format!("StrCpy $R7 {SETUP_EXE}"),
        "${If} $ClauseoAdminSectionRan = 1".to_owned(),
        format!("StrCpy $R7 {ADMIN_COPY}"),
        "${EndIf}".to_owned(),
        "${IfNot} ${FileExists} \"$R7\"".to_owned(),
        "DetailPrint \"$(setupExeRemoved)\"".to_owned(),
        "${If} ${Silent}".to_owned(),
        "SetErrorLevel ${CLAUSEO_EXIT_SETUP_EXE_REMOVED}".to_owned(),
        "${Else}".to_owned(),
        "MessageBox MB_ICONEXCLAMATION|MB_OK \"$(setupExeRemoved)\"".to_owned(),
        "${EndIf}".to_owned(),
        "${EndIf}".to_owned(),
        "Pop $R7".to_owned(),
    ];
    // The macro's last lines: nothing after it rolls the install back.
    assert_eq!(body[check..], expected, "{body:?}");
}

#[test]
fn the_installers_check_the_admin_copy_only_when_the_admin_section_ran() {
    let admin = admin_code();
    // Set in three places only, besides its declaration.
    let sets: Vec<&String> = admin
        .iter()
        .filter(|line| line.contains("ClauseoAdminSectionRan"))
        .collect();
    assert_eq!(
        sets,
        [
            "Var ClauseoAdminSectionRan",
            "StrCpy $ClauseoAdminSectionRan 1",
            "StrCpy $ClauseoAdminSectionRan 0",
            "StrCpy $ClauseoAdminSectionRan 1",
        ],
        "{admin:?}"
    );
    // Per user: once the elevated instance started (a declined prompt fails ShellExecuteExW), and
    // never in the /UPDATE, /S or /P branch, which ends at the interactive branch's ${Else}.
    let ask = macro_body(&admin, ASK);
    assert_run(
        ask,
        &[
            "System::Call 'shell32::ShellExecuteExW(p R5) i .R7 ?e'",
            "Pop $R9",
            "${If} $R7 <> 0",
            "StrCpy $ClauseoAdminSectionRan 1",
        ],
    );
    let skip = line_with(ask, &["${ElseIf} ${Silent}"]);
    let interactive = skip + line_with(&ask[skip..], &["${Else}"]);
    assert!(interactive < line_with(ask, &["StrCpy $ClauseoAdminSectionRan 1"]));
    // Cleared when the elevated instance found Clauseo running (97): it wrote nothing.
    assert_run(
        ask,
        &[
            "${ElseIf} $R8 = ${CLAUSEO_ADMIN_APP_RUNNING}",
            "StrCpy $R6 6",
            "StrCpy $ClauseoAdminSectionRan 0",
        ],
    );
    // Per machine: every run past the Clauseo-running check, updates included.
    let machine = macro_body(&admin, "CLAUSEO_ADMIN_COPY_MACHINE");
    assert_run(
        machine,
        &[
            "${Else}",
            "StrCpy $ClauseoAdminSectionRan 1",
            "!insertmacro CLAUSEO_RECORD_CHECK \"\"",
        ],
    );
    // The copy checked is the one the uninstaller and the app start.
    let dir = &admin[line_with(&admin, &["!define CLAUSEO_ADMIN_DIR "])];
    let dir = dir.strip_prefix("!define CLAUSEO_ADMIN_DIR \"").unwrap();
    let exe = format!(
        "{}\\clauseo-sandbox-setup.exe\"",
        dir.strip_suffix('"').unwrap()
    );
    let code = hook_code();
    assert_eq!(
        code[line_with(&code, &["!define CLAUSEO_ADMIN_EXE "])],
        format!("!define CLAUSEO_ADMIN_EXE \"{exe}")
    );
}

#[test]
fn the_uninstaller_says_the_protection_stays_when_the_set_up_program_is_gone() {
    let code = hook_code();
    let body = preuninstall(&code);
    let freed = line_with(body, &["System::Free $R6"]);
    let check = line_with(body, &["${IfNot} ${FileExists}", ADMIN_EXE]);
    let deletion = line_with(body, &["${If} $DeleteAppDataCheckboxState = 1"]);
    // After the removal's block (the class key's, the check's and the removal's own ends), before
    // the data folders.
    assert_eq!(
        body[freed + 1..check],
        [
            "System::Call 'advapi32::RegCloseKey(p R4)'",
            "${EndIf}",
            "${EndIf}",
            "${EndIf}",
        ],
        "{body:?}"
    );
    let expected = [
        format!("${{IfNot}} ${{FileExists}} {ADMIN_EXE}"),
        "SetRegView 64".to_owned(),
        "ClearErrors".to_owned(),
        "EnumRegValue $R3 HKLM \"SOFTWARE\\Clauseo\\Sandbox\" 0".to_owned(),
        "${IfNot} ${Errors}".to_owned(),
        "DetailPrint \"$(removeExeMissing)\"".to_owned(),
        "${If} ${Silent}".to_owned(),
        "SetErrorLevel ${CLAUSEO_EXIT_REMOVE_EXE_MISSING}".to_owned(),
        "${Else}".to_owned(),
        "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(removeExeMissing)\"".to_owned(),
        "${EndIf}".to_owned(),
        "${EndIf}".to_owned(),
        "SetRegView lastused".to_owned(),
        "${EndIf}".to_owned(),
    ];
    assert_eq!(body[check..check + expected.len()], expected, "{body:?}");
    assert_eq!(check + expected.len(), deletion, "{body:?}");
    // The key is the set-up's machine record, as the sandbox crate names it.
    let record = read("../sandbox/src/windows/setup/record_write.rs");
    assert!(
        record.contains("pub(super) const RECORD_KEY: &str = \"SOFTWARE\\\\Clauseo\\\\Sandbox\";"),
        "record_write.rs"
    );
}

#[test]
fn the_two_messages_are_the_drafts_and_name_no_path() {
    let removed = "\"Windows security removed part of Clauseo while it was installing: the \
                   program that sets up the Safe workspace. Until it's restored, Clauseo won't \
                   start Claude or ChatGPT in the Safe workspace. Whoever looks after this \
                   computer can restore it. Clauseo's notes for IT explain how.\"";
    let missing = "\"Clauseo couldn't remove the Safe workspace, because Windows security removed \
                   the program that sets it up. The Safe workspace's hidden Windows accounts are \
                   still on this computer. Whoever looks after this computer can remove them. \
                   Clauseo's notes for IT explain how.\"";
    assert_eq!(english("setupExeRemoved"), removed);
    assert_eq!(english("removeExeMissing"), missing);
    for text in [removed, missing] {
        assert!(!text.contains('$') && !text.contains('\\'), "{text}");
    }
    // The running copy lists them (ui/app/doc-approvals.json).
    let approvals: serde_json::Value =
        serde_json::from_str(&read("../../ui/app/doc-approvals.json")).unwrap();
    let lines = &approvals["crates/desktop/windows/nsis/English.nsh"]["lines"];
    for key in ["removeExeMissing", "setupExeRemoved"] {
        assert!(
            lines.as_array().unwrap().iter().any(|line| line == key),
            "{key}"
        );
    }
}

#[test]
fn the_silent_exit_codes_are_distinct_unused_and_listed_for_firm_it() {
    let code = hook_code();
    let installed = defined(&code, "CLAUSEO_EXIT_SETUP_EXE_REMOVED");
    let uninstalled = defined(&code, "CLAUSEO_EXIT_REMOVE_EXE_MISSING");
    let refused = defined(&code, "CLAUSEO_EXIT_ADMIN_COPY_REFUSED");
    assert_ne!(installed, uninstalled);
    assert!(![installed, uninstalled].contains(&refused));
    // The installers' admin-copy steps keep 22 to 26.
    assert!(!(22..=26).contains(&refused), "{refused}");
    // NSIS's own (0, 1, 2), and what Intune and SCCM read as success, a reboot or a retry.
    for code in [installed, uninstalled, refused] {
        assert!(
            ![0, 1, 2, 1223, 1618, 1641, 1707, 3010].contains(&code),
            "{code}"
        );
    }
    // Only these three set an exit code.
    let sets: Vec<&String> = code
        .iter()
        .filter(|line| line.contains("SetErrorLevel"))
        .collect();
    assert_eq!(
        sets,
        [
            "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_REFUSED}",
            "SetErrorLevel ${CLAUSEO_EXIT_REMOVE_EXE_MISSING}",
            "SetErrorLevel ${CLAUSEO_EXIT_SETUP_EXE_REMOVED}",
        ],
        "{code:?}"
    );
    let firm_it = read("../../docs/firm-it.md");
    assert!(
        firm_it
            .lines()
            .any(|line| line == "## 10. If Windows security removed Clauseo's set-up program"),
        "firm-it.md's section"
    );
    for (value, words) in [
        (
            installed,
            "| `/S` install: installed, but Windows security removed",
        ),
        (
            uninstalled,
            "| `/S` uninstall: uninstalled, but the protection stays",
        ),
        (
            refused,
            "| `/S` uninstall: uninstalled, but the protection stays",
        ),
    ] {
        let row = format!("| {value} {words}");
        assert!(firm_it.lines().any(|line| line.starts_with(&row)), "{row}");
    }
}
