//! The repo's NSIS template (`windows/nsis/installer.nsi`, `bundle.windows.nsis.template`) is
//! tauri-cli 2.12.2's own plus a known list of named blocks, and the one block there is
//! (`admin-section`, first in `.onInit`) runs the elevated branch of `admin_elevated.nsh`, which
//! loads no plug-in, calls no function and reads nothing the lawyer can write before it extracts.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use super::admin_copy::{admin_code, assert_run, macro_body};
use super::sources::{line_with, read};

/// The Tauri CLI whose template the copy follows: `ui/package.json` pins the CLI that bundles.
/// Cargo.lock has no tauri-cli or tauri-bundler, so the npm pin is what moves on an upgrade.
const TAURI_CLI: &str = "2.12.2";

/// SHA-256 of tauri-cli 2.12.2's `crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`
/// (tag tauri-cli-v2.12.2), 984 lines, LF, as the bundler embeds it.
const UPSTREAM_SHA256: &str = "166f9677d42e81c7f712a1bf80d9d8c089c21d2967d8398474768b5eac14b494";

/// The blocks the copy adds, in order: each one's name and the upstream line it follows directly.
/// A new block is one more row here (and its lines in the template).
const BLOCKS: [(&str, &str); 1] = [("admin-section", "Function .onInit")];

const TEMPLATE: &str = "windows/nsis/installer.nsi";

/// The template's blocks, each as (name, index of the line before it, its lines), and the text
/// with every block taken out. Panics on an unclosed or mismatched marker.
fn split_blocks(text: &str) -> (Vec<(String, usize, Vec<String>)>, String) {
    let mut blocks = Vec::new();
    let mut kept: Vec<&str> = Vec::new();
    let mut open: Option<(String, usize, Vec<String>)> = None;
    for line in text.split('\n') {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("; >>> clauseo: ") {
            assert!(open.is_none(), "a block opens inside another: {name}");
            open = Some((name.to_owned(), kept.len().wrapping_sub(1), Vec::new()));
        } else if let Some(name) = trimmed.strip_prefix("; <<< clauseo: ") {
            let block = open
                .take()
                .unwrap_or_else(|| panic!("{name} closes no block"));
            assert_eq!(block.0, name, "a block closes under another name");
            blocks.push(block);
        } else if let Some(block) = open.as_mut() {
            block.2.push(trimmed.to_owned());
        } else {
            kept.push(line);
        }
    }
    assert!(open.is_none(), "a block is never closed");
    (blocks, kept.join("\n"))
}

#[test]
fn the_template_is_tauri_cli_2_12_2s_plus_the_named_blocks() {
    let ui = read("../../ui/package.json");
    assert!(
        ui.contains(&format!("\"@tauri-apps/cli\": \"{TAURI_CLI}\",")),
        "the Tauri CLI moved: copy its installer.nsi again, put the blocks back, update the pin"
    );
    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    assert_eq!(conf["bundle"]["windows"]["nsis"]["template"], TEMPLATE);
    let text = read(TEMPLATE);
    let (blocks, upstream) = split_blocks(&text);
    let names: Vec<&str> = blocks.iter().map(|(name, _, _)| name.as_str()).collect();
    let known: Vec<&str> = BLOCKS.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, known);
    let upstream_lines: Vec<&str> = upstream.split('\n').collect();
    for ((name, before, _), (_, anchor)) in blocks.iter().zip(BLOCKS) {
        assert_eq!(
            upstream_lines[*before], anchor,
            "{name} follows the wrong line"
        );
    }
    let digest = Sha256::digest(upstream.as_bytes());
    let hex = digest.iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").unwrap();
        hex
    });
    assert_eq!(
        hex, UPSTREAM_SHA256,
        "the template differs from tauri-cli {TAURI_CLI}'s outside its named blocks"
    );
}

#[test]
fn the_admin_section_block_only_inserts_the_elevated_branch_first_in_on_init() {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    let (_, _, lines) = &blocks[0];
    let code: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .collect();
    assert_eq!(
        code,
        [
            "!ifmacrodef CLAUSEO_ELEVATED_BRANCH",
            "!insertmacro CLAUSEO_ELEVATED_BRANCH",
            "!endif",
        ]
    );
    // First in .onInit: the marker is the line right after `Function .onInit`.
    let text = read(TEMPLATE);
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines
        .iter()
        .position(|line| *line == "Function .onInit")
        .unwrap();
    assert_eq!(lines[start + 1].trim(), "; >>> clauseo: admin-section");
}

/// The macros `name` inserts, itself included, through every `!insertmacro` in `code`.
fn reached<'a>(code: &'a [String], name: &str) -> Vec<(String, &'a [String])> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![name.to_owned()];
    let mut out = Vec::new();
    while let Some(next) = todo.pop() {
        if !seen.insert(next.clone()) {
            continue;
        }
        let body = macro_body(code, &next);
        for line in body {
            if let Some(rest) = line.strip_prefix("!insertmacro ") {
                let inserted = rest.split_whitespace().next().unwrap();
                todo.push(inserted.to_owned());
            }
        }
        out.push((next, body));
    }
    out
}

#[test]
fn the_elevated_branch_loads_no_plug_in_calls_no_function_and_reads_nothing_the_lawyer_writes() {
    let code = admin_code();
    let macros = reached(&code, "CLAUSEO_ELEVATED_BRANCH");
    let names: BTreeSet<&str> = macros.iter().map(|(name, _)| name.as_str()).collect();
    for expected in [
        "CLAUSEO_PARSE_ARGUMENTS",
        "CLAUSEO_CHECK_SID",
        "CLAUSEO_CHECK_PROFILE",
        "CLAUSEO_CHECK_PATH",
        "CLAUSEO_RECORD_CHECK",
        "CLAUSEO_NEWER_THAN_THIS",
        "CLAUSEO_TAKE_NUMBER",
        "CLAUSEO_ADMIN_COPY",
    ] {
        assert!(names.contains(expected), "{expected}: {names:?}");
    }
    // LogicLib's words, which expand to plain jumps; anything else in ${...} is a define.
    let logic = [
        "If", "IfNot", "ElseIf", "Else", "EndIf", "AndIf", "OrIf", "Do", "Loop", "ExitDo", "Errors",
    ];
    let refused = [
        "::",
        "$TEMP",
        "$PLUGINSDIR",
        "InitPluginsDir",
        "ReadEnvStr",
        "ExpandEnvStrings",
        "HKCU",
        "SHCTX",
        "$LOCALAPPDATA",
        "$APPDATA",
        "$PROFILE",
        "$EXEDIR",
    ];
    for (name, body) in &macros {
        for line in *body {
            assert!(!line.starts_with("Call "), "{name}: {line}");
            for word in refused {
                assert!(!line.contains(word), "{name}: {word} in {line}");
            }
            for piece in line.split("${").skip(1) {
                let token = piece.split('}').next().unwrap();
                let define = token
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
                assert!(
                    define || logic.contains(&token),
                    "{name}: ${{{token}}} in {line}"
                );
            }
        }
    }
}

#[test]
fn the_elevated_branch_checks_its_arguments_and_the_record_then_quits() {
    let code = admin_code();
    let body = macro_body(&code, "CLAUSEO_ELEVATED_BRANCH");
    assert_run(
        body,
        &[
            "!insertmacro CLAUSEO_PARSE_ARGUMENTS",
            "${If} $R0 = 1",
            "SetRegView 64",
            "!insertmacro CLAUSEO_CHECK_SID $R2",
            "StrCpy $R4 \"\"",
            "${If} $R0 = 1",
            "ReadRegStr $R4 HKLM \"${CLAUSEO_PROFILE_LIST}\\$R2\" \"ProfileImagePath\"",
            "!insertmacro CLAUSEO_CHECK_PROFILE",
            "${EndIf}",
            "!insertmacro CLAUSEO_CHECK_PATH $INSTDIR",
            "!insertmacro CLAUSEO_CHECK_PATH $R3",
            "StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}",
            "${If} $R0 = 1",
            "!insertmacro CLAUSEO_RECORD_CHECK $R2",
            "${If} $R1 = 1",
            "IntOp $R8 ${CLAUSEO_ADMIN_EXIT_BASE} + ${CLAUSEO_ADOPT_OTHER_ACCOUNT}",
            "${ElseIf} $R1 = 2",
            "StrCpy $R8 ${CLAUSEO_ADMIN_NEWER_KEPT}",
            "${Else}",
            "!insertmacro CLAUSEO_ADMIN_COPY '--owner-sid $R2 --user-install-dir \"$INSTDIR\" \
             --data-dir \"$R3\"'",
            "${If} $R8 <> ${CLAUSEO_ADMIN_NOT_RUN}",
            "IntOp $R8 $R8 + ${CLAUSEO_ADMIN_EXIT_BASE}",
            "${EndIf}",
            "${EndIf}",
            "${EndIf}",
            "SetErrorLevel $R8",
            "Quit",
            "${EndIf}",
        ],
    );
    assert_eq!(body[1], "!if \"${INSTALLMODE}\" == \"currentUser\"");
    line_with(
        &code,
        &[
            "!define CLAUSEO_PROFILE_LIST \"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList\"",
        ],
    );
    // The switch counts only as a whole argument, compared exactly; the data folder takes the
    // rest of the line; the SID is its option's value.
    let parse = macro_body(&code, "CLAUSEO_PARSE_ARGUMENTS");
    line_with(parse, &["${If} $R4 S== \"${CLAUSEO_ADMIN_SWITCH}\""]);
    line_with(parse, &["${If} $R5 S== \"${CLAUSEO_DATA_DIR_OPTION}\""]);
    line_with(parse, &["${If} $R5 S== \"${CLAUSEO_OWNER_SID_OPTION}\""]);
    line_with(parse, &["StrCpy $R1 $CMDLINE"]);
    // The path checks: inside the profile, no "..", no quote, slash or wildcard, no trailing "\".
    let path = macro_body(&code, "CLAUSEO_CHECK_PATH");
    line_with(path, &["${OrIf} $R5 != \"$R4\\\""]);
    line_with(path, &["${AndIf} $R9 == \".\""]);
    line_with(path, &["${If} $R5 == \"\\\""]);
    let sid = macro_body(&code, "CLAUSEO_CHECK_SID");
    line_with(sid, &["${IfNot} $R5 S== \"S-1-\""]);
    line_with(sid, &["${AndIf} $R7 S!= $R5"]);
    let profile = macro_body(&code, "CLAUSEO_CHECK_PROFILE");
    line_with(profile, &["${If} $R5 == \"%\""]);
}
