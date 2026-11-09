//! The uninstall hook starts the set-up program's admin copy in Program Files only, never the
//! copy beside the app, and never falls back to it (S15 tier B, "Admin copy" in 0049). Before it
//! starts it, it checks it as the app does: the folder chain admin-only, then the copy's SHA-256,
//! computed in process, compared with the record's; a copy that fails is refused, never started,
//! and the refusal says the sandbox's accounts stay only to their owner. The start looks its
//! verb up in the machine's class for an exe alone.

use super::admin_copy::admin_code;
use super::nsis_run::{Machine, Node, Record, Value, chain, defines, lines, parse_all, preprocess};
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
fn the_hook_checks_the_chain_then_compares_the_hash_reading_the_record_again_on_a_mismatch() {
    let code = hook_code();
    let verified = macro_lines(&code, "CLAUSEO_ADMIN_COPY_VERIFIED");
    // $R4 starts refused; the chain, then the record's hash, then the copy's, then the compare.
    let seeded = line_with(verified, &["StrCpy $R4 0"]);
    let copy = line_with(verified, &["StrCpy $0 \"${CLAUSEO_ADMIN_EXE}\""]);
    let chain_check = line_with(verified, &["!insertmacro CLAUSEO_CHAIN_SAFE"]);
    let record = compare_at(verified);
    assert!(
        seeded < copy && copy < chain_check && chain_check < record,
        "{verified:?}"
    );
    // Read only once the chain passed; only a 64-character hash is compared; one that couldn't be
    // computed is 2; on a mismatch the record is read once more before the one compare.
    let expected = [
        "${If} $5 = 1",
        "SetRegView 64",
        "ReadRegStr $3 HKLM \"SOFTWARE\\Clauseo\\Sandbox\" \"AdminCopySha256\"",
        "SetRegView lastused",
        "StrLen $2 $3",
        "${If} $2 = 64",
        "!insertmacro CLAUSEO_SHA256_OF_COPY",
        "${If} $2 <> 1",
        "StrCpy $R4 2",
        "${Else}",
        "${If} $4 != $3",
        "SetRegView 64",
        "ReadRegStr $3 HKLM \"SOFTWARE\\Clauseo\\Sandbox\" \"AdminCopySha256\"",
        "SetRegView lastused",
        "${EndIf}",
        "${If} $4 == $3",
        "StrCpy $R4 1",
        "${EndIf}",
        "${EndIf}",
        "${EndIf}",
        "${EndIf}",
    ];
    assert_eq!(
        verified[record..record + expected.len()],
        expected,
        "{verified:?}"
    );
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
    // the removal): the compare above.
    let real = line_with(&code, &["!macro CLAUSEO_REAL_FOLDER"]);
    let real = real..real + macro_lines(&code, "CLAUSEO_REAL_FOLDER").len();
    let passes: Vec<usize> = (0..code.len())
        .filter(|&at| code[at] == "StrCpy $R4 1" && !real.contains(&at))
        .collect();
    assert_eq!(passes.len(), 1, "{code:?}");
    assert_eq!(code[passes[0] - 1], "${If} $4 == $3");
    // In the removal: the check right under the copy's test, and the start only in the branch
    // where it passed.
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
    assert_eq!(body[check + 1], "${If} $R4 <> 1", "{body:?}");
    let nodes = parse_all(body);
    let Some(Node::If(arms, Some(started))) = chain(&nodes, "If $R4 <> 1") else {
        panic!("no refusal with an ${{Else}}: {body:?}");
    };
    let start = "System::Call 'shell32::ShellExecuteExW(p R6) i .R7'";
    assert!(lines(&arms[0].1).iter().all(|line| *line != start));
    assert!(lines(started).contains(&start));
    let checks = body
        .iter()
        .filter(|line| line.contains("CLAUSEO_ADMIN_COPY_VERIFIED"));
    assert_eq!(checks.count(), 1, "{body:?}");
}

/// Where the record's hash is read in `CLAUSEO_ADMIN_COPY_VERIFIED`: the `${If} $5 = 1` (the chain
/// passed) that the 64-bit view follows.
fn compare_at(verified: &[String]) -> usize {
    (0..verified.len().saturating_sub(1))
        .find(|&at| verified[at] == "${If} $5 = 1" && verified[at + 1] == "SetRegView 64")
        .unwrap_or_else(|| panic!("no compare: {verified:?}"))
}

/// The compare of `CLAUSEO_ADMIN_COPY_VERIFIED`, from the record's read, run on `record`, with the
/// copy's hash computed as `hashed` (`None`: it couldn't be), and the record left as `refreshed`
/// once the hash is done, when given: $R4.
fn compared(record: Record, hashed: Option<&str>, refreshed: Option<Record>) -> String {
    let code = hook_code();
    let verified = macro_lines(&code, "CLAUSEO_ADMIN_COPY_VERIFIED");
    let nodes = parse_all(&verified[compare_at(verified)..]);
    let Some(compare) = nodes.first() else {
        panic!("no compare: {verified:?}");
    };
    let mut machine = Machine::new(defines(&code), record);
    let fake = match hashed {
        Some(hash) => vec!["StrCpy $2 1".to_owned(), format!("StrCpy $4 {hash}")],
        None => vec!["StrCpy $2 0".to_owned(), "StrCpy $4 \"\"".to_owned()],
    };
    machine
        .inline
        .insert("CLAUSEO_SHA256_OF_COPY".to_owned(), parse_all(&fake));
    if let Some(record) = refreshed {
        machine
            .after
            .insert("CLAUSEO_SHA256_OF_COPY".to_owned(), record);
    }
    machine.set_var("$5", "1");
    machine.set_var("$R4", "0");
    machine.run(std::slice::from_ref(compare));
    machine.vars["$R4"].clone()
}

#[test]
fn the_hash_is_compared_ignoring_case_and_again_after_a_refresh() {
    let a = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let b = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
    let sha = |hash: &'static str| Some(vec![("AdminCopySha256", Value::Text(hash))]);
    assert_eq!(compared(sha(a), Some(a), None), "1");
    assert_eq!(compared(sha(a), Some(&a.to_uppercase()), None), "1");
    // A mismatch stays one when the record is the same on the second read.
    assert_eq!(compared(sha(a), Some(b), None), "0");
    // A refresh that wrote a new copy and its hash between the first read and the hash: the
    // second read matches.
    assert_eq!(compared(sha(a), Some(b), Some(sha(b))), "1");
    // A hash that couldn't be computed is 2, whatever the record says.
    assert_eq!(compared(sha(a), None, None), "2");
    // No record, or not a 64-character one: refused (0), nothing hashed.
    for record in [
        None,
        sha("0123"),
        Some(vec![("AdminCopySha256", Value::Unreadable)]),
    ] {
        assert_eq!(compared(record, None, None), "0");
    }
}

#[test]
fn the_hook_hashes_the_copy_in_process_with_bcrypt_and_gives_every_handle_back() {
    let code = hook_code();
    let hash = macro_lines(&code, "CLAUSEO_SHA256_OF_COPY");
    // bcrypt from $SYSDIR by its full path; the file opened for reading, shared for reading only;
    // 64 KiB chunks into the hash; its 32 bytes as 64 hex digits.
    let expected = [
        "StrCpy $2 0",
        "StrCpy $4 \"\"",
        "StrCpy $R6 0",
        "System::Call '$SYSDIR\\bcrypt.dll::BCryptOpenAlgorithmProvider(*p .R6, w \"SHA256\", p 0, \
         i 0) i .r1'",
        "${If} $1 = 0",
        "StrCpy $R7 0",
        "System::Call '$SYSDIR\\bcrypt.dll::BCryptCreateHash(p R6, *p .R7, p 0, i 0, p 0, i 0, i \
         0) i .r1'",
        "${If} $1 = 0",
        "System::Call 'kernel32::CreateFileW(w r0, i 0x120089, i 1, p 0, i 3, i 0, p 0) p .R8'",
        "${If} $R8 <> -1",
        "System::Alloc 65536",
        "Pop $R9",
        "${If} $R9 <> 0",
        "StrCpy $5 1",
        "${Do}",
        "System::Call 'kernel32::ReadFile(p R8, p R9, i 65536, *i .r6, p 0) i .r1'",
        "${If} $1 = 0",
        "StrCpy $5 0",
        "${ExitDo}",
        "${EndIf}",
        "${If} $6 = 0",
        "${ExitDo}",
        "${EndIf}",
        "System::Call '$SYSDIR\\bcrypt.dll::BCryptHashData(p R7, p R9, i r6, i 0) i .r1'",
        "${If} $1 <> 0",
        "StrCpy $5 0",
        "${ExitDo}",
        "${EndIf}",
        "${Loop}",
        "${If} $5 = 1",
        "System::Call '$SYSDIR\\bcrypt.dll::BCryptFinishHash(p R7, p R9, i 32, i 0) i .r1'",
        "${If} $1 = 0",
        "${For} $5 0 31",
        "IntOp $6 $R9 + $5",
        "System::Call '*$6(&i1 .r7)'",
        "IntOp $7 $7 & 0xFF",
        "IntFmt $7 \"%02x\" $7",
        "StrCpy $4 \"$4$7\"",
        "${Next}",
        "StrCpy $2 1",
        "${EndIf}",
        "${EndIf}",
        "System::Free $R9",
        "${EndIf}",
        "System::Call 'kernel32::CloseHandle(p R8)'",
        "${EndIf}",
        "System::Call '$SYSDIR\\bcrypt.dll::BCryptDestroyHash(p R7)'",
        "${EndIf}",
        "System::Call '$SYSDIR\\bcrypt.dll::BCryptCloseAlgorithmProvider(p R6, i 0)'",
        "${EndIf}",
        "${If} $2 <> 1",
        "StrCpy $4 \"\"",
        "${EndIf}",
    ];
    assert_eq!(hash[1..], expected, "{hash:?}");
    // Each close is the last line of the arm its open's success test opens, so it runs on every
    // path past the open, and nothing leaves the macro early.
    let nodes = parse_all(&hash[1..]);
    for (open, close) in [
        (
            "BCryptOpenAlgorithmProvider",
            "BCryptCloseAlgorithmProvider(p R6, i 0)",
        ),
        ("BCryptCreateHash", "BCryptDestroyHash(p R7)"),
        ("kernel32::CreateFileW", "kernel32::CloseHandle(p R8)"),
        ("Pop $R9", "System::Free $R9"),
    ] {
        let arm = arm_after(&nodes, open).unwrap_or_else(|| panic!("{open}"));
        let Some(Node::Line(last)) = arm.last() else {
            panic!("{open}: the arm doesn't end with a line");
        };
        assert!(last.contains(close), "{open}: {last}");
    }
    for word in ["Goto", "Return", "Abort", "Quit"] {
        assert!(
            hash.iter().all(|line| !line.starts_with(word)),
            "{word}: {hash:?}"
        );
    }
    // No program is started for the hash, and nothing is written for it.
    for line in &code {
        let lower = line.to_lowercase();
        for banned in [
            "certutil",
            "createprocessw",
            "cmd.exe",
            "nsexec",
            "execdos",
            "crypto::",
            "admin-copy-sha256",
        ] {
            assert!(!lower.contains(banned), "{line}");
        }
    }
    let verified = macro_lines(&code, "CLAUSEO_ADMIN_COPY_VERIFIED");
    line_with(verified, &["!insertmacro CLAUSEO_SHA256_OF_COPY"]);
}

/// The first arm of the `${If}` right after the line holding `open`, at any depth.
fn arm_after<'a>(nodes: &'a [Node], open: &str) -> Option<&'a [Node]> {
    for (at, node) in nodes.iter().enumerate() {
        let found = match node {
            Node::Line(line) if line.contains(open) => {
                return match nodes.get(at + 1) {
                    Some(Node::If(arms, _)) => Some(&arms[0].1),
                    _ => None,
                };
            }
            Node::If(arms, otherwise) => arms
                .iter()
                .find_map(|(_, body)| arm_after(body, open))
                .or_else(|| otherwise.as_ref().and_then(|body| arm_after(body, open))),
            Node::Do(body) => arm_after(body, open),
            Node::Line(_) => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

const ME: &str = "S-1-5-21-1-2-3-1001";
const OTHER: &str = "S-1-5-21-1-2-3-1002";
const NO_CLAIM: &str = "the sandbox tool in Program Files failed its check, so it wasn't started";
const UNHASHED: &str = "couldn't compute the sandbox tool's hash";

/// The refusal (the hook's `${If} $R4 <> 1` arm) in the `mode` build, run with `verified` in $R4
/// on `record`, as the user `ME`, silent or not: what it said.
fn refusal(mode: &str, verified: &str, record: Record, silent: bool) -> Vec<String> {
    let code: Vec<String> = hook_code().into_iter().chain(admin_code()).collect();
    let mut defines = defines(&code);
    defines.insert("INSTALLMODE".to_owned(), mode.to_owned());
    defines.insert("NSIS_PTR_SIZE".to_owned(), "4".to_owned());
    let body = preprocess(preuninstall(&code), &defines);
    let nodes = parse_all(&body);
    let Some(Node::If(arms, _)) = chain(&nodes, "If $R4 <> 1") else {
        panic!("no refusal: {body:?}");
    };
    let tell = preprocess(&macro_lines(&code, "CLAUSEO_OWNER_TO_TELL")[1..], &defines);
    let mut machine = Machine::new(defines, record);
    machine
        .inline
        .insert("CLAUSEO_OWNER_TO_TELL".to_owned(), parse_all(&tell));
    machine.silent = silent;
    machine.own_sid = ME;
    machine.set_var("$R4", verified);
    machine.set_var("$R5", "0");
    machine.run(&arms[0].1);
    // It keeps the check's result and the removal's ($R5, read later), and its stack balanced.
    assert_eq!(machine.vars["$R4"], verified);
    assert_eq!(machine.vars["$R5"], "0");
    assert_eq!(machine.stack, Vec::<String>::new());
    machine.said
}

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

#[test]
fn the_refusal_says_the_accounts_stay_only_to_their_owner() {
    let owned_by = |sid: &'static str| Some(vec![("OwnerSid", Value::Text(sid))]);
    let machine_copy = Some(vec![("AdminCopyOwner", Value::Text("machine"))]);
    let claim = said(&[
        "DetailPrint $(removeAdminCopyRefused)",
        "MessageBox $(removeAdminCopyRefused)",
    ]);
    let claim_silent = said(&["DetailPrint $(removeAdminCopyRefused)", "SetErrorLevel 27"]);
    let no_claim = said(&[&format!("DetailPrint {NO_CLAIM}")]);
    let no_claim_silent = said(&[&format!("DetailPrint {NO_CLAIM}"), "SetErrorLevel 27"]);
    // Per user: to the owner, the claim (a box unless /S); to anyone else, or with no owner, a
    // details line that claims nothing, and no box.
    assert_eq!(refusal("currentUser", "0", owned_by(ME), false), claim);
    assert_eq!(
        refusal("currentUser", "0", owned_by(ME), true),
        claim_silent
    );
    for record in [owned_by(OTHER), machine_copy.clone(), None] {
        assert_eq!(refusal("currentUser", "0", record.clone(), false), no_claim);
        assert_eq!(refusal("currentUser", "0", record, true), no_claim_silent);
    }
    // Per machine: to anyone, when an owner is recorded; else the details line.
    assert_eq!(refusal("perMachine", "0", owned_by(OTHER), false), claim);
    assert_eq!(
        refusal("perMachine", "0", owned_by(OTHER), true),
        claim_silent
    );
    for record in [machine_copy, None] {
        assert_eq!(refusal("perMachine", "0", record.clone(), false), no_claim);
        assert_eq!(refusal("perMachine", "0", record, true), no_claim_silent);
    }
    // A hash that couldn't be computed says so first; the claim follows the same rule.
    let unhashed = format!("DetailPrint {UNHASHED}");
    let mut with_claim = said(&[&unhashed]);
    with_claim.extend(claim);
    assert_eq!(
        refusal("perMachine", "2", owned_by(OTHER), false),
        with_claim
    );
    assert_eq!(refusal("currentUser", "2", None, false), said(&[&unhashed]));
    assert_eq!(
        refusal("currentUser", "2", owned_by(OTHER), true),
        said(&[&unhashed, "SetErrorLevel 27"])
    );
    // The claim's words are the draft's; the other two are plain details lines.
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
    line_with(
        &hook_code(),
        &["!define CLAUSEO_EXIT_ADMIN_COPY_REFUSED 27"],
    );
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
    // One chain rule, for the admin copy and the previous install folder alike.
    for user in ["CLAUSEO_ADMIN_COPY_VERIFIED", "CLAUSEO_PREVIOUS_DIR_OK"] {
        line_with(
            macro_lines(&code, user),
            &["!insertmacro CLAUSEO_CHAIN_SAFE"],
        );
    }
    let levels = code
        .iter()
        .filter(|line| *line == "!insertmacro CLAUSEO_LEVEL_SAFE");
    assert_eq!(levels.count(), 1, "{code:?}");
    // The first level is the root (the path's first three characters) against the root's
    // rights; the last, the file, a file; every other a folder against the full rights.
    let verified = macro_lines(&code, "CLAUSEO_CHAIN_SAFE");
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
