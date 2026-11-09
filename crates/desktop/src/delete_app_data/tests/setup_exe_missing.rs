//! What the sources must keep when Windows security removed the sandbox's set-up program
//! (`clauseo-sandbox-setup.exe`). Launches run only the admin copy in Program Files, so the
//! installers say so (`setupExeRemoved`, exit code 20) only when this run wrote that copy and found
//! it gone when it tried to start `--adopt` (`CLAUSEO_ADMIN_REMOVED`, 95), never for a run that
//! wrote nothing or failed to write, and never for the copy in `$INSTDIR`. The uninstaller says so
//! when it finds its admin copy (the one its removal starts) gone while the record names a set-up's
//! owner: per user only to that owner, per machine whoever it is. Each code is one
//! `docs/firm-it.md` lists. One test per case the release-build review named.

use super::admin_copy::{ASK, admin_code, assert_run, block, macro_body};
use super::sources::{hook_code, line_with, preuninstall, read};

/// The admin copy in Program Files, the one the uninstaller's removal starts (`hooks.nsh`).
const ADMIN_EXE: &str = "\"${CLAUSEO_ADMIN_EXE}\"";
/// The same copy as `admin_copy.nsh` names it.
const ADMIN_COPY: &str = "\"${CLAUSEO_ADMIN_DIR}\\clauseo-sandbox-setup.exe\"";
const SAYS: &str = "CLAUSEO_ADMIN_COPY_MACHINE_SAYS";
const REMOVED_LINE: &str = "DetailPrint \"$(setupExeRemoved)\"";
const SET_20: &str = "SetErrorLevel ${CLAUSEO_EXIT_SETUP_EXE_REMOVED}";

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

/// The indexes of the lines in `body` that mention `setupExeRemoved` or set exit code 20.
fn says_removed(body: &[String]) -> Vec<usize> {
    (0..body.len())
        .filter(|&at| {
            body[at].contains("setupExeRemoved")
                || body[at].contains("CLAUSEO_EXIT_SETUP_EXE_REMOVED")
        })
        .collect()
}

#[test]
fn the_new_code_is_free_and_kept_clear_of_the_offset() {
    let code = admin_code();
    line_with(&code, &["!define CLAUSEO_ADMIN_REMOVED 95"]);
    // Beside the other internal codes, none of which it repeats, and below 100 + --adopt's code,
    // so the elevated instance passes it on as it is.
    for (name, value) in [
        ("NOT_RUN", 99),
        ("NEWER_KEPT", 98),
        ("APP_RUNNING", 97),
        ("TIMED_OUT", 96),
    ] {
        line_with(&code, &[&format!("!define CLAUSEO_ADMIN_{name} {value}")]);
    }
    let branch = macro_body(&code, "CLAUSEO_ELEVATED_BRANCH");
    assert_run(
        branch,
        &[
            "${If} $R8 <> ${CLAUSEO_ADMIN_NOT_RUN}",
            "${AndIf} $R8 <> ${CLAUSEO_ADMIN_REMOVED}",
            "IntOp $R8 $R8 + ${CLAUSEO_ADMIN_EXIT_BASE}",
            "${EndIf}",
        ],
    );
}

#[test]
fn the_per_user_copy_and_the_attempt_flag_are_gone() {
    let code = hook_code();
    let body = postinstall(&code);
    // The post-install macro ends with the mode block: no check of $INSTDIR's copy after it.
    assert_eq!(body.last().map(String::as_str), Some("!endif"), "{body:?}");
    let all: Vec<String> = code.iter().cloned().chain(admin_code()).collect();
    for line in &all {
        assert!(!line.contains("ClauseoAdminSectionRan"), "{line}");
        assert!(
            !(line.contains("FileExists") && line.contains("$INSTDIR\\clauseo-sandbox-setup.exe")),
            "{line}"
        );
    }
    assert!(
        code.iter().all(|line| !line.contains("setupExeRemoved")),
        "hooks.nsh says nothing of a removed set-up program"
    );
}

#[test]
fn a_declined_or_failed_elevation_or_97_says_nothing_of_removal() {
    let code = admin_code();
    let ask = macro_body(&code, ASK);
    // 1223 is outcome 4 (declined), 1260 and 5 outcome 9 (blocked), 97 outcome 6, any other
    // failure (no elevated instance, 99, NSIS's 1 or 2) outcome 1 through -1 and ${CaseElse}.
    // Only outcome 8 says it, and only 95 makes outcome 8.
    assert_run(ask, &["${ElseIf} $R9 = 1223", "StrCpy $R6 4"]);
    assert_run(
        ask,
        &[
            "${ElseIf} $R8 = ${CLAUSEO_ADMIN_APP_RUNNING}",
            "StrCpy $R6 6",
            "${ElseIf} $R8 = ${CLAUSEO_ADMIN_REMOVED}",
            "StrCpy $R6 8",
        ],
    );
    assert_run(ask, &["${CaseElse}", "StrCpy $R6 1"]);
    let eights = ask.iter().filter(|line| *line == "StrCpy $R6 8").count();
    assert_eq!(eights, 1, "{ask:?}");
    let shown = line_with(ask, &["${ElseIf} $R6 = 8"]);
    assert_eq!(
        says_removed(ask),
        [shown + 1, shown + 2, shown + 3],
        "{ask:?}"
    );
}

#[test]
fn a_per_machine_run_with_clauseo_running_says_nothing_of_removal() {
    let code = admin_code();
    let machine = macro_body(&code, "CLAUSEO_ADMIN_COPY_MACHINE");
    let running = block(machine, "${If} $R5 <> 0", "${Else}");
    assert!(says_removed(running).is_empty(), "{running:?}");
    // Nothing else in the macro says it: only the outcome macro, after a copy was written.
    assert!(says_removed(machine).is_empty(), "{machine:?}");
}

#[test]
fn an_update_with_the_per_user_copy_gone_and_the_admin_copy_there_says_nothing() {
    let code = admin_code();
    let ask = macro_body(&code, ASK);
    // Per user, /UPDATE does nothing at all: its branch is empty up to the /S and /P one.
    assert_run(
        ask,
        &[
            "${If} $UpdateMode = 1",
            "${ElseIf} ${Silent}",
            "${OrIf} $PassiveMode = 1",
        ],
    );
    // Per machine, 95 comes only from a failed start of the copy just written, and only when that
    // copy is gone; the copy in $INSTDIR is never looked at (the test above).
    let bounded = macro_body(&code, "CLAUSEO_ADMIN_COPY_BOUNDED");
    assert_run(
        bounded,
        &[
            "System::Call 'kernel32::CloseHandle(p r1)'",
            &format!("${{ElseIfNot}} ${{FileExists}} {ADMIN_COPY}"),
            "StrCpy $R8 ${CLAUSEO_ADMIN_REMOVED}",
            "${EndIf}",
        ],
    );
}

#[test]
fn a_copy_written_then_removed_says_so_and_exits_20() {
    let code = admin_code();
    // Per user: the elevated instance quits with 95 when ExecWait couldn't start the copy and it
    // is gone; the outer instance shows the box, the details line and 20.
    let copy = macro_body(&code, "CLAUSEO_ADMIN_COPY");
    assert_run(
        copy,
        &[
            "${If} ${Errors}",
            "StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}",
            &format!("${{IfNot}} ${{FileExists}} {ADMIN_COPY}"),
            "StrCpy $R8 ${CLAUSEO_ADMIN_REMOVED}",
            "${EndIf}",
        ],
    );
    let ask = macro_body(&code, ASK);
    assert_run(
        ask,
        &[
            "${ElseIf} $R6 = 8",
            REMOVED_LINE,
            "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(setupExeRemoved)\"",
            SET_20,
        ],
    );
    // Per machine: the outcome macro says it for 95, and 20 is set outside the box's mode test.
    let says = macro_body(&code, SAYS);
    assert_run(
        says,
        &[
            "${ElseIf} $R8 = ${CLAUSEO_ADMIN_REMOVED}",
            REMOVED_LINE,
            "${If} $PassiveMode <> 1",
            SET_20,
            "${EndIf}",
        ],
    );
}

#[test]
fn a_failed_write_exits_23_and_never_20() {
    let code = admin_code();
    // 95 is set only inside the extract's ${IfNot} ${Errors}: a folder or file that couldn't be
    // written leaves 99 (NOT_RUN), which is a failure, as are NSIS's 1 and 2.
    for name in ["CLAUSEO_ADMIN_COPY", "CLAUSEO_ADMIN_COPY_BOUNDED"] {
        let body = macro_body(&code, name);
        assert_run(
            body,
            &[
                "StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}",
                "!insertmacro CLAUSEO_ADMIN_EXTRACT",
                "${IfNot} ${Errors}",
            ],
        );
        let ok = line_with(body, &["${IfNot} ${Errors}"]);
        let removed = line_with(body, &["StrCpy $R8 ${CLAUSEO_ADMIN_REMOVED}"]);
        assert!(ok < removed, "{name}: {body:?}");
    }
    // Per machine, MachineFailed (99 included) sets 23 under /S and nothing says removal there.
    let says = macro_body(&code, SAYS);
    let failed = line_with(says, &["DetailPrint \"$(adminCopyMachineFailed)\""]);
    assert_run(
        &says[failed..],
        &[
            "DetailPrint \"$(adminCopyMachineFailed)\"",
            "${If} ${Silent}",
            "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_FAILED}",
        ],
    );
    assert!(says_removed(&says[failed..]).is_empty(), "{says:?}");
    // Exit code 20 is set in two places only: outcome 8 per user and 95 per machine.
    let sets = code.iter().filter(|line| *line == SET_20).count();
    assert_eq!(sets, 2, "{code:?}");
}

#[test]
fn a_passive_install_writes_a_details_line_and_shows_no_box_or_code() {
    let code = admin_code();
    // Per machine: the details line in every mode, 20 unless /P, the box only interactively.
    let says = macro_body(&code, SAYS);
    let expected = [
        "${ElseIf} $R8 = ${CLAUSEO_ADMIN_REMOVED}",
        REMOVED_LINE,
        "${If} $PassiveMode <> 1",
        SET_20,
        "${EndIf}",
        "${IfNot} ${Silent}",
        "${AndIf} $PassiveMode <> 1",
        "${AndIf} $UpdateMode <> 1",
        "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(setupExeRemoved)\"",
        "${EndIf}",
    ];
    assert_run(says, &expected);
    // Per user, /P never elevates: outcome 8 is past the /S and /P branch's ${Else}.
    let ask = macro_body(&code, ASK);
    let skip = line_with(ask, &["${ElseIf} ${Silent}"]);
    let interactive = skip + line_with(&ask[skip..], &["${Else}"]);
    assert!(says_removed(&ask[..interactive]).is_empty(), "{ask:?}");
}

/// The uninstaller's check for a missing set-up program: from its `${IfNot} ${FileExists}` of the
/// admin copy to the matching `${EndIf}`, which the data folders' step follows.
fn exe_missing(body: &[String]) -> &[String] {
    let check = line_with(body, &["${IfNot} ${FileExists}", ADMIN_EXE]);
    let deletion = line_with(body, &["${If} $DeleteAppDataCheckboxState = 1"]);
    // After the removal's block (the class key's, the check's and the removal's own ends), before
    // the data folders.
    assert_eq!(body[check - 5], "System::Free $R6", "{body:?}");
    assert_eq!(
        body[check - 4..check],
        [
            "System::Call 'advapi32::RegCloseKey(p R4)'",
            "${EndIf}",
            "${EndIf}",
            "${EndIf}",
        ],
        "{body:?}"
    );
    // The check's own end, then the end of the Safe workspace's `!ifdef CLAUSEO_SAFE_WORKSPACE`,
    // which holds the removal and this check (safe_workspace_off.rs).
    assert_eq!(
        body[deletion - 2..deletion],
        ["${EndIf}", "!endif"],
        "{body:?}"
    );
    &body[check..deletion - 1]
}

/// The message lines, gated on a non-empty `$R3` (the owner's SID, cleared when it isn't said).
const SAYS_MISSING: [&str; 9] = [
    "${If} $R3 != \"\"",
    "DetailPrint \"$(removeExeMissing)\"",
    "${If} ${Silent}",
    "SetErrorLevel ${CLAUSEO_EXIT_REMOVE_EXE_MISSING}",
    "${Else}",
    "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(removeExeMissing)\"",
    "${EndIf}",
    "${EndIf}",
    "${EndIf}",
];

#[test]
fn the_uninstaller_says_nothing_when_the_key_names_no_owner() {
    let code = hook_code();
    let check = exe_missing(preuninstall(&code));
    // Who to tell (CLAUSEO_OWNER_TO_TELL, which the refusal of a copy that fails its check uses
    // too), then the message only behind a non-empty $R3.
    assert_eq!(check[0], format!("${{IfNot}} ${{FileExists}} {ADMIN_EXE}"));
    assert_eq!(check[1], "!insertmacro CLAUSEO_OWNER_TO_TELL", "{check:?}");
    assert_eq!(check[2..], SAYS_MISSING, "{check:?}");
    // OwnerSid read, not the key tested: --adopt --per-machine writes the key with nobody set up,
    // and a removal deletes OwnerSid but leaves the admin copy's values.
    let tell = macro_body(&code, "CLAUSEO_OWNER_TO_TELL");
    assert_eq!(
        tell[1..4],
        [
            "SetRegView 64",
            "ReadRegStr $R3 HKLM \"${CLAUSEO_RECORD_KEY}\" \"${CLAUSEO_RECORD_OWNER_SID}\"",
            "SetRegView lastused",
        ],
        "{tell:?}"
    );
    assert!(
        tell.iter().all(|line| !line.contains("EnumRegValue")),
        "{tell:?}"
    );
    assert_eq!(
        check
            .iter()
            .filter(|line| line.contains("removeExeMissing"))
            .count(),
        2
    );
    // The key and value are the set-up's, as the sandbox crate names them; a removal deletes
    // OwnerSid (SETUP_VALUES) and keeps the admin copy's values.
    let admin = admin_code();
    line_with(
        &admin,
        &["!define CLAUSEO_RECORD_KEY \"SOFTWARE\\Clauseo\\Sandbox\""],
    );
    line_with(&admin, &["!define CLAUSEO_RECORD_OWNER_SID \"OwnerSid\""]);
    let record = read("../sandbox/src/windows/setup/record_write.rs");
    assert!(
        record.contains("pub(super) const RECORD_KEY: &str = \"SOFTWARE\\\\Clauseo\\\\Sandbox\";"),
        "record_write.rs"
    );
    let values = read("../sandbox/src/windows/setup/record_values.rs");
    assert!(
        values.contains("pub const OWNER_SID: &str = \"OwnerSid\";"),
        "record_values.rs"
    );
    // OwnerSid comes first among the set-up's values, which a removal deletes, however many
    // values the list holds (the stop's `ResetOwed` and the install's `InstallId` among them).
    let list = values
        .split_once("pub const SETUP_VALUES: [&str; ")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(count, rest)| (count.parse::<usize>(), rest));
    assert!(
        matches!(list, Some((Ok(_), rest)) if rest.starts_with(" = [\n    OWNER_SID,")),
        "record_values.rs"
    );
}

/// The per-user comparison: the uninstalling user's SID from its own token against `OwnerSid`.
const SAME_USER: [&str; 16] = [
    "!if \"${INSTALLMODE}\" == \"currentUser\"",
    "${If} $R3 != \"\"",
    "Push $R4",
    "Push $R5",
    "Push $R6",
    "Push $R7",
    "!insertmacro CLAUSEO_OWN_SID",
    "Pop $R7",
    "Pop $R6",
    "Pop $R5",
    "Pop $R4",
    "${If} $R2 S!= $R3",
    "StrCpy $R3 \"\"",
    "${EndIf}",
    "${EndIf}",
    "!endif",
];

#[test]
fn a_per_user_uninstall_by_the_owner_says_so_with_21() {
    let code = hook_code();
    let check = exe_missing(preuninstall(&code));
    // $R3 keeps OwnerSid when it equals the uninstalling user's SID, last in the macro, then the
    // message and 21.
    let tell = macro_body(&code, "CLAUSEO_OWNER_TO_TELL");
    let compare = line_with(tell, &["!if \"${INSTALLMODE}\" == \"currentUser\""]);
    assert_eq!(tell[compare..], SAME_USER, "{tell:?}");
    assert_eq!(check[2..], SAYS_MISSING, "{check:?}");
    // The SID is this process's own token user, read natively through System::Call.
    let admin = admin_code();
    let own = macro_body(&admin, "CLAUSEO_OWN_SID");
    line_with(own, &["advapi32::OpenProcessToken(p -1, i 8, *p .R4)"]);
    line_with(own, &["advapi32::ConvertSidToStringSidW"]);
}

#[test]
fn a_per_user_uninstall_by_another_user_says_nothing() {
    let code = hook_code();
    // Another SID clears $R3, so the message's gate fails; $R4 (the copy's check) and $R5 (the
    // removal's result) are kept.
    let tell = macro_body(&code, "CLAUSEO_OWNER_TO_TELL");
    assert_run(tell, &["${If} $R2 S!= $R3", "StrCpy $R3 \"\"", "${EndIf}"]);
    let own = line_with(tell, &["!insertmacro CLAUSEO_OWN_SID"]);
    for register in ["$R4", "$R5"] {
        let pushed = line_with(tell, &[&format!("Push {register}")]);
        let popped = line_with(tell, &[&format!("Pop {register}")]);
        assert!(pushed < own && own < popped, "{tell:?}");
    }
}

#[test]
fn a_per_machine_uninstall_with_an_owner_says_so() {
    let code = hook_code();
    let check = exe_missing(preuninstall(&code));
    // The comparison is the currentUser build's only: per machine, the read goes straight to the
    // message, whoever the owner is.
    let tell = macro_body(&code, "CLAUSEO_OWNER_TO_TELL");
    let read_owner = line_with(tell, &["ReadRegStr $R3 HKLM"]);
    assert_eq!(tell[read_owner + 1], "SetRegView lastused");
    assert_eq!(tell[read_owner + 2], SAME_USER[0]);
    assert_eq!(tell.len(), read_owner + 2 + SAME_USER.len(), "{tell:?}");
    assert_eq!(check[2..], SAYS_MISSING, "{check:?}");
    assert_eq!(
        tell.iter().filter(|line| line.starts_with("!if ")).count(),
        1
    );
}

#[test]
fn the_two_messages_are_the_drafts_and_name_no_path() {
    let removed = "\"Windows security removed part of Clauseo while it was installing: the \
                   program that sets up the Safe workspace. Until it's restored, Clauseo won't \
                   start Claude or ChatGPT in the Safe workspace. Run this installer again to \
                   restore it. If Windows security removes it again, Clauseo's notes for IT say \
                   what to allow.\"";
    let missing = "\"Clauseo couldn't remove the Safe workspace, because Windows security removed \
                   the program that sets it up. The Safe workspace's hidden Windows accounts are \
                   still on this computer. Run Clauseo's installer again to restore that program, \
                   then uninstall Clauseo again. If that doesn't work, Clauseo's notes for IT \
                   explain how to remove them.\"";
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
    // The installers' and the per-machine uninstaller's 22 to 26 (admin_copy_ask.rs).
    assert!(!(22..=26).contains(&refused), "{refused}");
    // NSIS's own (0, 1, 2), and what Intune and SCCM read as success, a reboot or a retry.
    for code in [installed, uninstalled, refused] {
        assert!(
            ![0, 1, 2, 1223, 1618, 1641, 1707, 3010].contains(&code),
            "{code}"
        );
    }
    // In hooks.nsh only the uninstaller's two set one (21 and 27); admin_copy.nsh sets 20 (the
    // tests above).
    let sets: Vec<&String> = code
        .iter()
        .filter(|line| line.contains("SetErrorLevel"))
        .collect();
    assert_eq!(
        sets,
        [
            "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_REFUSED}",
            "SetErrorLevel ${CLAUSEO_EXIT_REMOVE_EXE_MISSING}",
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
            "| Install: installed, but this run wrote `clauseo-sandbox-setup.exe`",
        ),
        (
            uninstalled,
            "| `/S` uninstall: uninstalled, but the protection stays",
        ),
        (
            refused,
            "| Per-machine `/S` uninstall: uninstalled, but the protection stays",
        ),
    ] {
        let row = format!("| {value} {words}");
        assert!(firm_it.lines().any(|line| line.starts_with(&row)), "{row}");
    }
}
