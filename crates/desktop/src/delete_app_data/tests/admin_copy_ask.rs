//! The admin copy's per-user ask and its outcomes (`admin_copy.nsh`): the outer, unelevated
//! instance reads the lawyer's SID from its own token, starts itself elevated once and waits for
//! it, never in update, silent or passive mode; every exit code of `--adopt` has its outcome; and
//! the words are the provisional ones. The rest is in `admin_copy.rs`.

use super::admin_copy::{ASK, admin_code, assert_run, block, code_of, macro_body};
use super::nsis_run::{Machine, chain, defines, parse_all};
use super::sources::{hook_code, line_with, read};

#[test]
fn the_outer_instance_reads_its_own_sid_from_its_token() {
    let code = admin_code();
    let body = macro_body(&code, "CLAUSEO_OWN_SID");
    let order = [
        "advapi32::OpenProcessToken(p -1, i 8, *p .R4) i .R5",
        "advapi32::GetTokenInformation(p R4, i 1, p R6, i 256, *i .R5) i .R7",
        "advapi32::ConvertSidToStringSidW(p R5, *p .R7) i .R5",
        "kernel32::lstrcpynW(w .R2, p R7, i ${NSIS_MAX_STRLEN})",
        "kernel32::LocalFree(p R7)",
        "kernel32::CloseHandle(p R4)",
    ];
    let lines: Vec<usize> = order.iter().map(|part| line_with(body, &[part])).collect();
    assert!(lines.windows(2).all(|pair| pair[0] < pair[1]), "{body:?}");
}

#[test]
fn the_per_user_installer_asks_once_by_starting_itself_elevated_and_never_in_update_or_silent() {
    let code = hook_code();
    let post = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    // The mode block's last lines: the ask, then repair mode's relaunch (repair.rs); only the
    // set-up program check (setup_exe_missing.rs) follows.
    let end = post.iter().rposition(|line| line == "!endif").unwrap() + 1;
    let tail: Vec<&str> = post[end - 4..end].iter().map(String::as_str).collect();
    assert_eq!(
        tail,
        [
            "!else",
            &format!("!insertmacro {ASK}"),
            "!insertmacro CLAUSEO_REPAIR_RELAUNCH",
            "!endif"
        ],
        "{post:?}"
    );
    let admin = admin_code();
    let body = macro_body(&admin, ASK);
    // Update first, doing nothing; then /S and /P, a details line and, for /S, the exit code.
    let skips = [
        "${If} $UpdateMode = 1",
        "${ElseIf} ${Silent}",
        "${OrIf} $PassiveMode = 1",
        "DetailPrint \"$(adminCopySkippedSilent)\"",
        "${If} ${Silent}",
        "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_SKIPPED}",
        "${EndIf}",
        "${Else}",
    ];
    assert_run(body, &skips);
    let interactive = line_with(body, &["${If} $UpdateMode = 1"]) + skips.len();
    // The lawyer's SID and data folder, from this instance; the folder made when missing; then
    // the announcement; then the one elevation: this installer, "runas", the switch and both
    // values, /S, and /D= last (NSIS reads the rest of the line as the folder).
    assert_run(
        body,
        &[
            "!insertmacro CLAUSEO_OWN_SID",
            "StrCpy $R3 \"$LOCALAPPDATA\\${BUNDLEID}\"",
            "CreateDirectory \"$R3\"",
            "FindWindow $R5 \"${CLAUSEO_MAIN_WINDOW_CLASS}\"",
            "${If} $R5 <> 0",
            "StrCpy $R6 6",
            "StrCpy $R2 \"\"",
            "${EndIf}",
            "${If} $R2 != \"\"",
            "MessageBox MB_OK|MB_ICONINFORMATION|MB_SETFOREGROUND|MB_TOPMOST \"$(adminCopyAsk)\"",
        ],
    );
    let calls: Vec<usize> = (0..body.len())
        .filter(|&i| body[i].contains("ShellExecuteExW"))
        .collect();
    assert_eq!(calls.len(), 1, "{body:?}");
    let call = calls[0];
    assert!(interactive < line_with(body, &["!insertmacro CLAUSEO_OWN_SID"]));
    assert!(line_with(body, &["$(adminCopyAsk)"]) < call, "{body:?}");
    let info = &body[call - 1];
    for part in [
        "w \"runas\"",
        "w \"$EXEPATH\"",
        "w \"/S ${CLAUSEO_ADMIN_SWITCH} ${CLAUSEO_OWNER_SID_OPTION}$R2 \
         ${CLAUSEO_DATA_DIR_OPTION}$R3 /D=$INSTDIR\"",
        "w \"$SYSDIR\"",
        "i 0x800440",
    ] {
        assert!(info.contains(part), "{part}: {info}");
    }
    // A declined prompt is its own outcome; the wait has no end but the instance's own.
    // Any other failed launch names its code; elevation this computer's settings refuse (1260,
    // or 5 under some UAC settings), never a person's No (1223), is outcome 9.
    assert_run(
        body,
        &[
            "${ElseIf} $R9 = 1223",
            "StrCpy $R6 4",
            "${Else}",
            "DetailPrint \"Windows refused the elevation (error $R9)\"",
            "${If} $R9 = 1260",
            "${OrIf} $R9 = 5",
            "StrCpy $R6 9",
            "${EndIf}",
        ],
    );
    // Up to 10 minutes, then outcome 7 (a details line).
    line_with(
        body,
        &["WaitForSingleObject(p R7, i ${CLAUSEO_ADMIN_WAIT_MS}) i .R9"],
    );
    assert_run(body, &["${If} $R9 = 258", "StrCpy $R6 7"]);
    assert_run(
        body,
        &["${If} $R7 <> 0", "DetailPrint \"$(adminCopyWorking)\""],
    );
    assert!(
        body[..interactive]
            .iter()
            .all(|line| !line.contains("MessageBox")),
        "{body:?}"
    );
}

#[test]
fn the_installers_exit_codes_are_their_own() {
    let code: Vec<String> = hook_code()
        .into_iter()
        .chain(admin_code())
        .chain(code_of("windows/uninstall_in_place.nsh"))
        .collect();
    // Every installer and uninstaller exit code the hooks define, by its !define line.
    let codes: Vec<(String, u32)> = code
        .iter()
        .filter_map(|line| line.strip_prefix("!define CLAUSEO_EXIT_"))
        .map(|rest| {
            let (name, value) = rest.split_once(' ').unwrap();
            (name.to_owned(), value.parse().unwrap())
        })
        .collect();
    let admin: Vec<&(String, u32)> = codes
        .iter()
        .filter(|(name, _)| {
            name.starts_with("ADMIN_COPY_") || name == "APP_RUNNING" || name == "TEMP_FAILED"
        })
        .collect();
    assert_eq!(
        admin,
        [
            &("ADMIN_COPY_REFUSED".to_owned(), 27),
            &("ADMIN_COPY_SKIPPED".to_owned(), 22),
            &("ADMIN_COPY_FAILED".to_owned(), 23),
            &("APP_RUNNING".to_owned(), 24),
            &("TEMP_FAILED".to_owned(), 25)
        ]
    );
    // Each code once: #638's 20 and 21, these, the per-machine uninstaller's 26, and the
    // installers' 28 when Clauseo.exe is gone (app_exe_missing.rs).
    let mut values: Vec<u32> = codes.iter().map(|(_, value)| *value).collect();
    values.sort_unstable();
    values.dedup();
    assert_eq!(values.len(), codes.len(), "{codes:?}");
    assert_eq!(values, [20, 21, 22, 23, 24, 25, 26, 27, 28], "{codes:?}");
    // Clear of NSIS's 0 to 2, of #638's 20 and 21 (packaging-setup-exe-missing), of the codes
    // Intune and SCCM read as success or retry, and of the elevated instance's 99 and 100 + code.
    let taken = [0, 1, 2, 20, 21, 99, 1223, 1618, 1641, 1707, 3010];
    for (name, value) in admin {
        assert!(!taken.contains(value), "{name}");
        assert!(!(100..200).contains(value), "{name}");
    }
}

#[test]
fn every_exit_code_of_adopt_has_its_outcome() {
    let code = admin_code();
    // SetupExit's codes (S15 PR 2), the elevated instance's offset and its "not run".
    let adopt = [
        ("DONE", 0),
        ("USAGE", 1),
        ("OTHER_ACCOUNT", 2),
        ("FAILED", 3),
        ("LOCK_HELD", 5),
        ("DATA_DIR", 6),
        ("NOT_ELEVATED", 7),
        ("NOT_ADMIN_COPY", 8),
        ("DOWNGRADE", 9),
    ];
    for (name, value) in adopt {
        line_with(&code, &[&format!("!define CLAUSEO_ADOPT_{name} {value}")]);
    }
    line_with(&code, &["!define CLAUSEO_ADMIN_EXIT_BASE 100"]);
    line_with(&code, &["!define CLAUSEO_ADMIN_NOT_RUN 99"]);
    line_with(&code, &["!define CLAUSEO_ADMIN_NEWER_KEPT 98"]);
    let body = macro_body(&code, ASK);
    assert_run(
        body,
        &[
            "${If} $R8 = ${CLAUSEO_ADMIN_NEWER_KEPT}",
            "StrCpy $R6 5",
            "${ElseIf} $R8 = ${CLAUSEO_ADMIN_APP_RUNNING}",
            "StrCpy $R6 6",
            "${ElseIf} $R8 = ${CLAUSEO_ADMIN_REMOVED}",
            "StrCpy $R6 8",
            "${EndIf}",
            "${If} $R6 <> 4",
            "${AndIf} $R6 <> 5",
            "${AndIf} $R6 <> 6",
            "${AndIf} $R6 <> 7",
            "${AndIf} $R6 <> 8",
            "${AndIf} $R6 <> 9",
        ],
    );
    line_with(
        body,
        &["GetExitCodeProcess(p R7, *i ${CLAUSEO_ADMIN_NOT_RUN} R8)"],
    );
    // --adopt's own code only from 100 + code; anything else (99, NSIS's 1 or 2) is -1.
    assert_run(
        body,
        &[
            "StrCpy $R9 -1",
            "${If} $R8 >= ${CLAUSEO_ADMIN_EXIT_BASE}",
            "${AndIf} $R8 < 200",
            "IntOp $R9 $R8 - ${CLAUSEO_ADMIN_EXIT_BASE}",
            "${EndIf}",
        ],
    );
    // Each code named in one ${Case} line, with its outcome on the next: 0 set up, 2 another
    // account, the rest failed; and anything else failed.
    let select = block(body, "${Select} $R9", "${EndSelect}");
    for (name, _) in adopt {
        let define = format!("${{CLAUSEO_ADOPT_{name}}}");
        let cases: Vec<usize> = (0..select.len())
            .filter(|&i| select[i].starts_with("${Case") && select[i].contains(&define))
            .collect();
        assert_eq!(cases.len(), 1, "{name}: {select:?}");
        let outcome = match name {
            "DONE" => "StrCpy $R6 0",
            "OTHER_ACCOUNT" => "StrCpy $R6 2",
            "DOWNGRADE" => "StrCpy $R6 5",
            _ => "StrCpy $R6 1",
        };
        assert_eq!(select[cases[0] + 1], outcome, "{name}: {select:?}");
    }
    let other = line_with(select, &["${CaseElse}"]);
    assert_eq!(select[other + 1], "StrCpy $R6 1", "{select:?}");
    // The outcomes: a details line when set up; a box and a details line otherwise.
    assert_run(
        body,
        &[
            "${If} $R6 = 0",
            "DetailPrint \"$(adminCopyDone)\"",
            "${ElseIf} $R6 = 1",
            "DetailPrint \"$(adminCopyFailed)\"",
            "MessageBox MB_OK|MB_ICONEXCLAMATION|MB_SETFOREGROUND|MB_TOPMOST \"$(adminCopyFailed)\"",
            "${ElseIf} $R6 = 2",
            "DetailPrint \"$(adminCopyOtherAccount)\"",
            "MessageBox MB_OK|MB_ICONEXCLAMATION|MB_SETFOREGROUND|MB_TOPMOST \"$(adminCopyOtherAccount)\"",
            "${ElseIf} $R6 = 4",
            "DetailPrint \"$(adminCopyDeclined)\"",
            "MessageBox MB_OK|MB_ICONEXCLAMATION|MB_SETFOREGROUND|MB_TOPMOST \"$(adminCopyDeclined)\"",
            "${ElseIf} $R6 = 5",
            "DetailPrint \"$(adminCopyNewerKept)\"",
            "${ElseIf} $R6 = 6",
            "DetailPrint \"$(adminCopyAppRunning)\"",
            "MessageBox MB_OK|MB_ICONEXCLAMATION|MB_SETFOREGROUND|MB_TOPMOST \"$(adminCopyAppRunning)\"",
            "SetErrorLevel ${CLAUSEO_EXIT_APP_RUNNING}",
            "${ElseIf} $R6 = 7",
            "DetailPrint \"$(adminCopyStillRunning)\"",
            "${ElseIf} $R6 = 8",
            "DetailPrint \"$(setupExeRemoved)\"",
            "MessageBox MB_OK|MB_ICONEXCLAMATION|MB_SETFOREGROUND|MB_TOPMOST \"$(setupExeRemoved)\"",
            "SetErrorLevel ${CLAUSEO_EXIT_SETUP_EXE_REMOVED}",
            "${ElseIf} $R6 = 9",
            "DetailPrint \"$(adminCopyBlocked)\"",
            "MessageBox MB_OK|MB_ICONEXCLAMATION|MB_SETFOREGROUND|MB_TOPMOST \"$(adminCopyBlocked)\"",
            "${EndIf}",
        ],
    );
    // A failed start or no SID never reads a code: $R6 stays 1 from before the call.
    let seeded = line_with(body, &["StrCpy $R6 1"]);
    assert!(
        seeded < line_with(body, &["ShellExecuteExW(p R5)"]),
        "{body:?}"
    );
}

/// What the per-user ask says when its elevated launch fails with `code` (`ShellExecuteExW`'s
/// last error, popped into `$R9`): the launch's `${If} $R7 <> 0` chain and the outcome chain,
/// run as written, with `$R6` seeded 1 and `$R8` "not run", as the macro seeds them. The
/// `--adopt` mapping between them leaves `$R6` alone here: 99 is -1, `${CaseElse}`, outcome 1,
/// which `every_exit_code_of_adopt_has_its_outcome` holds.
fn says_after_a_failed_launch(code: u32) -> Vec<String> {
    let admin = admin_code();
    let nodes = parse_all(&macro_body(&admin, ASK)[1..]);
    let launch = chain(&nodes, "If $R7 <> 0").expect("the launch's chain");
    let outcome = chain(&nodes, "If $R6 = 0").expect("the outcome chain");
    let mut machine = Machine::new(defines(&admin), None);
    machine.set_var("$R6", "1");
    machine.set_var("$R7", "0");
    machine.set_var("$R8", "99");
    machine.set_var("$R9", &code.to_string());
    machine.run(std::slice::from_ref(launch));
    machine.run(std::slice::from_ref(outcome));
    machine.said
}

#[test]
fn a_failed_launch_says_blocked_declined_or_failed_by_its_code() {
    let refused = |code: u32| format!("DetailPrint Windows refused the elevation (error {code})");
    // 1260, the VM's measured code for a policy-refused elevation (a standard user with
    // ConsentPromptBehaviorUser 0), and 5 under other UAC settings: adminCopyBlocked.
    for code in [1260, 5] {
        assert_eq!(
            says_after_a_failed_launch(code),
            [
                refused(code),
                "DetailPrint $(adminCopyBlocked)".to_owned(),
                "MessageBox $(adminCopyBlocked)".to_owned(),
            ],
            "{code}"
        );
    }
    // 1223, a person's No: adminCopyDeclined, and no code line.
    assert_eq!(
        says_after_a_failed_launch(1223),
        [
            "DetailPrint $(adminCopyDeclined)",
            "MessageBox $(adminCopyDeclined)"
        ]
    );
    // Anything else: adminCopyFailed, with its code.
    for code in [2, 1155, 1261] {
        assert_eq!(
            says_after_a_failed_launch(code),
            [
                refused(code),
                "DetailPrint $(adminCopyFailed)".to_owned(),
                "MessageBox $(adminCopyFailed)".to_owned(),
            ],
            "{code}"
        );
    }
    // Outcome 9 keeps --adopt's code out of it.
    let admin = admin_code();
    line_with(macro_body(&admin, ASK), &["${AndIf} $R6 <> 9"]);
}

#[test]
fn the_words_about_the_admin_copy_are_the_provisional_ones() {
    let english = read("windows/nsis/English.nsh");
    for (key, words) in [
        (
            "adminCopyAsk",
            "Next, Windows asks for permission to set up the Safe workspace. It will say the \
             publisher is unknown. That's expected: it's this installer asking. Choose Yes.",
        ),
        (
            "adminCopyWorking",
            "Setting up the Safe workspace. This can take a few minutes.",
        ),
        ("adminCopyDone", "The Safe workspace is set up."),
        (
            "adminCopyDeclined",
            "Clauseo is installed, but the Safe workspace isn't set up, because permission was \
             refused when Windows asked. Clauseo works without it. To set it up, run this \
             installer again and choose Yes. If Windows asks for an administrator's password, ask \
             whoever looks after this computer to enter it.",
        ),
        (
            "adminCopyBlocked",
            "Clauseo is installed, but the Safe workspace isn't set up, because your Windows \
             account isn't allowed to approve this change. To set it up, ask whoever looks after \
             this computer to run this installer with an administrator account, or to install \
             Clauseo for all users.",
        ),
        (
            "adminCopyFailed",
            "Clauseo is installed, but the Safe workspace couldn't be set up. Clauseo works \
             without it. Run this installer again to try once more. If it still isn't set up, ask \
             whoever looks after this computer.",
        ),
        (
            "adminCopyOtherAccount",
            "Clauseo is installed, but the Safe workspace isn't set up, because another Windows \
             account on this computer already has it. Clauseo works without it.",
        ),
        (
            "adminCopyMachineFailed",
            "Clauseo is installed, but the Safe workspace couldn't be set up on this computer. \
             Clauseo works without it. Whoever looks after this computer can run this installer \
             again to set it up.",
        ),
        (
            "adminCopyNewerKept",
            "Clauseo kept the newer copy of the Safe workspace's set-up program that's already on \
             this computer.",
        ),
        (
            "adminCopyAppRunning",
            "Clauseo is installed, but the Safe workspace isn't set up, because Clauseo is \
             running. Quit Clauseo, then run this installer again.",
        ),
        (
            "adminCopyStillRunning",
            "Clauseo may still be setting up the Safe workspace. To see whether it worked, open \
             Clauseo's Settings and look under Safe workspace.",
        ),
        (
            "adminCopySkippedSilent",
            "The Safe workspace isn't set up, because this install is silent. To set it up, run \
             this installer again without /S.",
        ),
    ] {
        let line = format!("LangString {key} ${{LANG_ENGLISH}} \"{words}\"");
        assert!(english.lines().any(|l| l == line), "{line}");
    }
}
