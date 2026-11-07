//! The uninstall hook starts the set-up program's admin copy in Program Files only, never the
//! copy beside the app, and never falls back to it (S15 tier B, "Admin copy" in 0049). Before it
//! starts it, it checks it as the app does: the folder chain admin-only, then the copy's SHA-256
//! compared with the record's; a copy that fails is refused, never started. The start looks its
//! verb up in the machine's class for an exe alone.

use super::sources::{hook_code, line_with, preuninstall, read};

#[test]
fn the_hook_starts_only_the_admin_copy_and_never_the_one_beside_the_app() {
    let code = hook_code();
    let body = preuninstall(&code);
    // No code line of the removal names the set-up program but through the define: no copy
    // beside the app, and no fallback to one when the admin copy is missing.
    let named: Vec<&String> = body
        .iter()
        .filter(|l| l.contains("clauseo-sandbox-setup"))
        .filter(|l| !l.starts_with("!define CLAUSEO_ADMIN_EXE "))
        .collect();
    assert!(named.is_empty(), "{named:?}");
    // The define's value is the admin copy's path in Program Files, exactly.
    let define = line_with(&code, &["!define CLAUSEO_ADMIN_EXE "]);
    assert_eq!(
        code[define],
        "!define CLAUSEO_ADMIN_EXE \"$PROGRAMFILES64\\Clauseo\\Sandbox\\clauseo-sandbox-setup.exe\""
    );
    // The SHELLEXECUTEINFOW struct's lpFile (the member after lpVerb) is that define.
    let info = line_with(body, &["System::Call '*(&l4, i 0x443"]);
    assert!(
        body[info].contains(concat!(
            "w \"${CLAUSEO_REMOVE_VERB}\", w \"${CLAUSEO_ADMIN_EXE}\", ",
            "w \"${CLAUSEO_REMOVE}\", p 0,"
        )),
        "{}",
        body[info]
    );
}

/// The code lines of the macro `name`, from its `!macro` line to the line before `!macroend`.
fn macro_lines<'a>(code: &'a [String], name: &str) -> &'a [String] {
    let start = line_with(code, &[&format!("!macro {name}")]);
    let end = start + line_with(&code[start..], &["!macroend"]);
    &code[start..end]
}

#[test]
fn the_hook_checks_the_chain_hashes_compares_then_elevates_and_a_mismatch_refuses() {
    let code = hook_code();
    let verified = macro_lines(&code, "CLAUSEO_ADMIN_COPY_VERIFIED");
    // $R4 starts refused; the chain, then the record's hash, then certutil's, then the compare.
    let seeded = line_with(verified, &["StrCpy $R4 0"]);
    let copy = line_with(verified, &["StrCpy $0 \"${CLAUSEO_ADMIN_EXE}\""]);
    let chain = line_with(verified, &["!insertmacro CLAUSEO_LEVEL_SAFE"]);
    let record = line_with(
        verified,
        &["ReadRegStr $3 HKLM \"SOFTWARE\\Clauseo\\Sandbox\" \"AdminCopySha256\""],
    );
    let hashed = line_with(verified, &["!insertmacro CLAUSEO_CERTUTIL_HASH"]);
    let compared = line_with(verified, &["!insertmacro CLAUSEO_HASH_LINE_MATCHES"]);
    assert!(
        seeded < copy && copy < chain && chain < record && record < hashed && hashed < compared,
        "{verified:?}"
    );
    // The record is read only once the chain passed, and only a 64-character hash is compared;
    // the compare only once certutil ended.
    assert_eq!(verified[record - 2], "${If} $5 = 1", "{verified:?}");
    assert_eq!(verified[record + 3], "${If} $2 = 64", "{verified:?}");
    assert_eq!(verified[compared - 1], "${If} $2 = 1", "{verified:?}");
    // What the macro borrows it gives back, in reverse.
    let pushed: Vec<&str> = verified
        .iter()
        .filter_map(|line| line.strip_prefix("Push "))
        .collect();
    let mut popped: Vec<&str> = verified
        .iter()
        .filter_map(|line| line.strip_prefix("Pop "))
        .collect();
    popped.reverse();
    assert_eq!(pushed, popped);
    assert!(!pushed.contains(&"$R4") && !pushed.contains(&"$R5"));
    // $R4 becomes 1 in one place (CLAUSEO_REAL_FOLDER's own $R4 aside, the data folder's, after
    // the removal): a line of certutil's output, spaces dropped, is the record's.
    let real = line_with(&code, &["!macro CLAUSEO_REAL_FOLDER"]);
    let real = real..real + macro_lines(&code, "CLAUSEO_REAL_FOLDER").len();
    let passes: Vec<usize> = (0..code.len())
        .filter(|&at| code[at] == "StrCpy $R4 1" && !real.contains(&at))
        .collect();
    assert_eq!(passes.len(), 1, "{code:?}");
    assert_eq!(code[passes[0] - 1], "${If} $8 == $3");
    let matches = macro_lines(&code, "CLAUSEO_HASH_LINE_MATCHES");
    assert!(matches.contains(&code[passes[0]]), "{matches:?}");
    // In the removal: the check right under the copy's test, its refusal before anything starts,
    // and the start only in the branch where it passed.
    let body = preuninstall(&code);
    let check = line_with(body, &["!insertmacro CLAUSEO_ADMIN_COPY_VERIFIED"]);
    assert_eq!(
        body[check - 2..check],
        [
            "${If} $R3 = 1",
            "${AndIf} ${FileExists} \"${CLAUSEO_ADMIN_EXE}\""
        ],
        "{body:?}"
    );
    let refusal = [
        "${If} $R4 <> 1",
        "DetailPrint \"$(removeAdminCopyRefused)\"",
        "${If} ${Silent}",
        "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_REFUSED}",
        "${Else}",
        "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(removeAdminCopyRefused)\"",
        "${EndIf}",
        "${Else}",
    ];
    assert_eq!(body[check + 1..=check + refusal.len()], refusal, "{body:?}");
    let start = line_with(body, &["shell32::ShellExecuteExW(p R6) i .R7"]);
    let refused_or_started = check + refusal.len();
    assert!(refused_or_started < start, "{body:?}");
    let checks = body
        .iter()
        .filter(|line| line.contains("CLAUSEO_ADMIN_COPY_VERIFIED"));
    assert_eq!(checks.count(), 1, "{body:?}");
    // The exit code and the words are firm IT's and the drafts'.
    line_with(&code, &["!define CLAUSEO_EXIT_ADMIN_COPY_REFUSED 27"]);
    let english = read("windows/nsis/English.nsh");
    let words = "LangString removeAdminCopyRefused ${LANG_ENGLISH} \"Clauseo couldn't remove the \
                 Safe workspace, because it couldn't confirm that the program that sets it up, in \
                 Program Files, is the one Clauseo installed and that only administrators can \
                 change it. The Safe workspace's hidden Windows accounts are still on this \
                 computer. Whoever looks after this computer can remove them. Clauseo's notes for \
                 IT explain how.\"";
    assert!(english.lines().any(|line| line == words), "{words}");
    let approvals: serde_json::Value =
        serde_json::from_str(&read("../../ui/app/doc-approvals.json")).unwrap();
    let lines = &approvals["crates/desktop/windows/nsis/English.nsh"]["lines"];
    assert!(
        lines
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line == "removeAdminCopyRefused")
    );
}

#[test]
fn the_hook_hashes_with_certutil_into_its_own_temporary_folder() {
    let code = hook_code();
    let hash = macro_lines(&code, "CLAUSEO_CERTUTIL_HASH");
    let verified = macro_lines(&code, "CLAUSEO_ADMIN_COPY_VERIFIED");
    // The output file under $PLUGINSDIR, the uninstaller's own, made first.
    let made = line_with(verified, &["InitPluginsDir"]);
    let file = line_with(
        verified,
        &["StrCpy $4 \"$PLUGINSDIR\\admin-copy-sha256.txt\""],
    );
    assert!(made < file, "{verified:?}");
    // certutil from System32 (SysWOW64 for this 32-bit uninstaller), started directly, with the
    // copy ($0) and SHA256; never through cmd.exe or a plug-in that runs commands.
    let command = line_with(hash, &["StrCpy $1 "]);
    assert_eq!(
        hash[command],
        "StrCpy $1 '\"$SYSDIR\\certutil.exe\" -hashfile \"$0\" SHA256'"
    );
    let started = line_with(
        hash,
        &["kernel32::CreateProcessW(w \"$SYSDIR\\certutil.exe\", w r1,"],
    );
    assert!(command < started, "{hash:?}");
    for line in &code {
        let lower = line.to_lowercase();
        for banned in ["cmd.exe", "nsexec", "execdos", "crypto::"] {
            assert!(!lower.contains(banned), "{line}");
        }
    }
    // Waited for, bounded; only an ended wait (0) lets the output be compared.
    let wait = line_with(
        hash,
        &["kernel32::WaitForSingleObject(p R6, i 60000) i .r1"],
    );
    assert_eq!(hash[wait + 1..wait + 3], ["${If} $1 = 0", "StrCpy $2 1"]);
}

#[test]
fn the_hooks_chain_rule_runs_from_the_drives_root_down() {
    // The rights and the trusted owners are the app's: the sandbox crate's
    // admin_copy::chain_tests holds the defines to its constants.
    let code = hook_code();
    for name in ["CLAUSEO_CHANGE_RIGHTS", "CLAUSEO_ROOT_CHANGE_RIGHTS"] {
        line_with(&code, &[&format!("!define {name} 0x")]);
    }
    let trusted = macro_lines(&code, "CLAUSEO_TRUSTED_SID");
    for sid in ["S-1-5-18", "S-1-5-32-544", "${CLAUSEO_TRUSTED_INSTALLER}"] {
        line_with(trusted, &[&format!("$R8 == \"{sid}\"")]);
    }
    // The first level is the root (the path's first three characters) against the root's
    // rights; the last, the copy, a file; every other a folder against the full rights.
    let verified = macro_lines(&code, "CLAUSEO_ADMIN_COPY_VERIFIED");
    let level = line_with(verified, &["StrCpy $4 $0 $3"]);
    let expected = [
        "StrCpy $4 $0 $3",
        "StrCpy $6 ${CLAUSEO_CHANGE_RIGHTS}",
        "StrCpy $7 16",
        "${If} $3 = 2",
        "StrCpy $4 $0 3",
        "StrCpy $6 ${CLAUSEO_ROOT_CHANGE_RIGHTS}",
        "${ElseIf} $3 = $2",
        "StrCpy $7 0",
        "${EndIf}",
        "!insertmacro CLAUSEO_LEVEL_SAFE",
    ];
    assert_eq!(
        verified[level..level + expected.len()],
        expected,
        "{verified:?}"
    );
    // A level: its own attributes (a folder or file, no reparse point), a trusted owner, a DACL,
    // and its allow entries' rights against $6, inherit-only ones skipped.
    let safe = macro_lines(&code, "CLAUSEO_LEVEL_SAFE");
    for part in [
        "IntOp $1 $1 & 1040",
        "${OrIf} $9 = 0",
        "!insertmacro CLAUSEO_TRUSTED_SID $8",
        "IntOp $R8 $R8 & 8",
        "IntOp $R9 $R9 & $6",
        "!insertmacro CLAUSEO_TRUSTED_SID $R6",
    ] {
        line_with(safe, &[part]);
    }
}

#[test]
fn the_hooks_start_looks_its_verb_up_in_hklm_alone() {
    let code = hook_code();
    let body = preuninstall(&code);
    // HKEY_LOCAL_MACHINE (0x80000002), read-only (KEY_READ 0x20019), into $R4, and the start
    // only once it opened.
    let opened = line_with(body, &["advapi32::RegOpenKeyExW("]);
    assert_eq!(
        body[opened],
        "System::Call 'advapi32::RegOpenKeyExW(p 0x80000002, w \"SOFTWARE\\Classes\\exefile\", \
         i 0, i 0x20019, *p .R4) i .R2'"
    );
    assert_eq!(body[opened + 1], "${If} $R2 = 0", "{body:?}");
    // hkeyClass, the struct's twelfth member, is that key, and fMask has SEE_MASK_CLASSKEY.
    let info = line_with(body, &["System::Call '*(&l4, i 0x443"]);
    let members: Vec<&str> = body[info]
        .split_once("'*(")
        .and_then(|(_, rest)| rest.split_once(") p .R6'"))
        .map(|(members, _)| members.split(", ").collect())
        .unwrap();
    assert_eq!(members.len(), 15, "{members:?}");
    assert_eq!(members[11], "p R4", "{members:?}");
    assert_eq!(members[10], "p 0", "no class name: {members:?}");
    assert!(opened < info, "{body:?}");
    let closed = line_with(body, &["System::Call 'advapi32::RegCloseKey(p R4)'"]);
    let start = line_with(body, &["shell32::ShellExecuteExW(p R6) i .R7"]);
    assert!(start < closed, "{body:?}");
    // No other hive is ever named: HKEY_CLASSES_ROOT merges the user's classes in.
    for line in &code {
        for other in [
            "0x80000000",
            "0x80000001",
            "HKCR",
            "HKCU\\Software\\Classes",
        ] {
            assert!(!line.contains(other), "{line}");
        }
    }
}
