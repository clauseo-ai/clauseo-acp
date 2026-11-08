//! The repo's NSIS template (`windows/nsis/installer.nsi`, `bundle.windows.nsis.template`) is
//! tauri-cli 2.12.2's own plus a known list of named blocks: `no-solid` (no /SOLID, so nothing is
//! unpacked into %TEMP%), `finish-show` and `finish-show-function` (the finish page hides "Run
//! Clauseo" after a wait that timed out) and `admin-section` (first in `.onInit`), which runs the
//! elevated branch of `admin_elevated.nsh` and the per-machine installer's start
//! (`safe_temp.nsh`, tested in `safe_temp.rs`); that branch loads no plug-in, calls no function
//! and reads nothing the lawyer can write before it extracts. Clauseo's main window class is the
//! one the branch looks for. `ui-language`, later in `.onInit`, sets the installer's language
//! from the app's (`hooks.nsh`, tested in `installer_language.rs`). The per-machine
//! uninstaller's blocks (`uninstall-string`, `un-admin-section`, `uninstall-self-delete` and
//! `un-gui-end`) are tested in `uninstall_in_place.rs`. `signed-plugins` and `sign-uninstaller`
//! (S20a, Authenticode) are tested in `signing_blocks.rs`.

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

/// A block the copy adds: its name, the upstream line it follows directly, and the upstream line
/// it stands in for (none when it only adds lines).
struct Block {
    name: &'static str,
    after: &'static str,
    replaces: Option<&'static str>,
}

/// The blocks, in the template's order. A new block is one more row here (and its lines in the
/// template).
const BLOCKS: [Block; 11] = [
    Block {
        name: "no-solid",
        after: "  ; Set the compression algorithm. We default to LZMA.",
        replaces: Some("  SetCompressor /SOLID \"{{compression}}\""),
    },
    Block {
        name: "signed-plugins",
        after: "!addplugindir \"${ADDITIONALPLUGINSPATH}\"",
        replaces: None,
    },
    Block {
        name: "sign-uninstaller",
        after: "!endif",
        replaces: None,
    },
    Block {
        name: "finish-show",
        after: "!define MUI_FINISHPAGE_RUN_FUNCTION RunMainBinary",
        replaces: None,
    },
    Block {
        name: "finish-show-function",
        after: "!insertmacro MUI_PAGE_FINISH",
        replaces: None,
    },
    Block {
        name: "admin-section",
        after: "Function .onInit",
        replaces: None,
    },
    Block {
        name: "ui-language",
        after: "  ${EndIf}",
        replaces: None,
    },
    Block {
        name: "uninstall-string",
        after: "  WriteRegStr SHCTX \"${UNINSTKEY}\" \"InstallLocation\" \"$\\\"$INSTDIR$\\\"\"",
        replaces: Some(
            "  WriteRegStr SHCTX \"${UNINSTKEY}\" \"UninstallString\" \"$\\\"$INSTDIR\\uninstall.exe$\\\"\"",
        ),
    },
    Block {
        name: "un-admin-section",
        after: "Function un.onInit",
        replaces: None,
    },
    Block {
        name: "uninstall-self-delete",
        after: "  !endif",
        replaces: None,
    },
    Block {
        name: "un-gui-end",
        after: "SectionEnd",
        replaces: None,
    },
];

pub(super) const TEMPLATE: &str = "windows/nsis/installer.nsi";

/// A block found in the template: its name, the index in the kept lines where it stood, and its
/// lines (trimmed, markers left out).
pub(super) struct Found {
    pub(super) name: String,
    pub(super) at: usize,
    pub(super) lines: Vec<String>,
}

/// The template's blocks, and its lines with every block taken out. Panics on an unclosed or
/// mismatched marker.
pub(super) fn split_blocks(text: &str) -> (Vec<Found>, Vec<String>) {
    let mut blocks = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    let mut open: Option<Found> = None;
    for line in text.split('\n') {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("; >>> clauseo: ") {
            assert!(open.is_none(), "a block opens inside another: {name}");
            open = Some(Found {
                name: name.to_owned(),
                at: kept.len(),
                lines: Vec::new(),
            });
        } else if let Some(name) = trimmed.strip_prefix("; <<< clauseo: ") {
            let block = open
                .take()
                .unwrap_or_else(|| panic!("{name} closes no block"));
            assert_eq!(block.name, name, "a block closes under another name");
            blocks.push(block);
        } else if let Some(block) = open.as_mut() {
            block.lines.push(trimmed.to_owned());
        } else {
            kept.push(line.to_owned());
        }
    }
    assert!(open.is_none(), "a block is never closed");
    (blocks, kept)
}

/// The block named `name`.
pub(super) fn block_named<'a>(blocks: &'a [Found], name: &str) -> &'a Found {
    blocks
        .iter()
        .find(|block| block.name == name)
        .unwrap_or_else(|| panic!("no block {name}"))
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
    let (blocks, kept) = split_blocks(&text);
    let names: Vec<&str> = blocks.iter().map(|block| block.name.as_str()).collect();
    let known: Vec<&str> = BLOCKS.iter().map(|block| block.name).collect();
    assert_eq!(names, known);
    // Upstream again: each replacing block's line put back where the block stood.
    let mut upstream = kept;
    for (found, block) in blocks.iter().zip(&BLOCKS).rev() {
        assert_eq!(
            upstream[found.at - 1],
            block.after,
            "{} follows the wrong line",
            block.name
        );
        if let Some(line) = block.replaces {
            upstream.insert(found.at, line.to_owned());
        }
        // A line ending in a backslash would join the next one in NSIS.
        for line in &found.lines {
            assert!(!line.ends_with('\\'), "{}: {line}", block.name);
        }
    }
    let digest = Sha256::digest(upstream.join("\n").as_bytes());
    let hex = digest.iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").unwrap();
        hex
    });
    assert_eq!(
        hex, UPSTREAM_SHA256,
        "the template differs from tauri-cli {TAURI_CLI}'s outside its named blocks"
    );
}

/// The code lines of the block named `name`, comments and blank lines left out.
pub(super) fn block_code<'a>(blocks: &'a [Found], name: &str) -> Vec<&'a str> {
    block_named(blocks, name)
        .lines
        .iter()
        .map(String::as_str)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .collect()
}

#[test]
fn no_solid_compresses_each_file_on_its_own() {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    assert_eq!(
        block_code(&blocks, "no-solid"),
        ["SetCompressor \"{{compression}}\""]
    );
}

#[test]
fn the_finish_page_hides_run_clauseo_after_a_wait_that_timed_out() {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    assert_eq!(
        block_code(&blocks, "finish-show"),
        ["!define MUI_PAGE_CUSTOMFUNCTION_SHOW ClauseoFinishShow"]
    );
    assert_eq!(
        block_code(&blocks, "finish-show-function"),
        [
            "Function ClauseoFinishShow",
            "!ifmacrodef CLAUSEO_FINISH_SHOW",
            "!insertmacro CLAUSEO_FINISH_SHOW",
            "!endif",
            "FunctionEnd",
        ]
    );
    let code = admin_code();
    assert_eq!(
        macro_body(&code, "CLAUSEO_FINISH_SHOW")[1..],
        [
            "${If} $ClauseoAdminTimedOut = 1",
            "${NSD_Uncheck} $mui.FinishPage.Run",
            "ShowWindow $mui.FinishPage.Run ${SW_HIDE}",
            "${EndIf}",
        ]
    );
    line_with(&code, &["Var ClauseoAdminTimedOut"]);
    // Set by both waits when they time out: the outer instance's and the per-machine one's.
    let ask = macro_body(&code, "CLAUSEO_ASK_FOR_THE_ADMIN_SECTION");
    assert_run(
        ask,
        &[
            "${If} $R9 = 258",
            "StrCpy $R6 7",
            "StrCpy $ClauseoAdminTimedOut 1",
        ],
    );
    let says = macro_body(&code, "CLAUSEO_ADMIN_COPY_MACHINE_SAYS");
    assert_run(
        says,
        &[
            "DetailPrint \"$(adminCopyStillRunning)\"",
            "StrCpy $ClauseoAdminTimedOut 1",
        ],
    );
}

#[test]
fn the_admin_section_block_only_inserts_the_elevated_branch_first_in_on_init() {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    let code: Vec<&str> = block_named(&blocks, "admin-section")
        .lines
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
            "!ifmacrodef CLAUSEO_MACHINE_START",
            "!insertmacro CLAUSEO_MACHINE_START",
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
pub(super) fn reached<'a>(code: &'a [String], name: &str) -> Vec<(String, &'a [String])> {
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

/// What the elevated branch may not hold: a plug-in call, %TEMP% and the plug-in folder, the
/// environment, the lawyer's registry (HKCR merges in HKCU) and folders, and file reads.
const REFUSED: [&str; 18] = [
    "::",
    "$TEMP",
    "$PLUGINSDIR",
    "InitPluginsDir",
    "ReadEnvStr",
    "ExpandEnvStrings",
    "HKCU",
    "HKCR",
    "HKU",
    "HKEY_USERS",
    "HKEY_CURRENT_USER",
    "SHCTX",
    "$LOCALAPPDATA",
    "$APPDATA",
    "$PROFILE",
    "$EXEDIR",
    "ReadINIStr",
    "FileOpen",
];

/// The first of `REFUSED` that `line` holds.
pub(super) fn refused_word(line: &str) -> Option<&'static str> {
    REFUSED.into_iter().find(|word| line.contains(word))
}

#[test]
fn the_refused_words_catch_the_lawyers_registry_and_files() {
    assert_eq!(
        refused_word("ReadRegStr $R5 HKCR \"Software\\Clauseo\" \"x\""),
        Some("HKCR")
    );
    assert_eq!(
        refused_word("ReadINIStr $R5 \"$R3\\a.ini\" s k"),
        Some("ReadINIStr")
    );
    assert_eq!(
        refused_word("ReadRegStr $R4 HKLM \"${CLAUSEO_PROFILE_LIST}\\$R2\" \"ProfileImagePath\""),
        None
    );
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
        "CLAUSEO_SAFE_PLUGINSDIR",
    ] {
        assert!(names.contains(expected), "{expected}: {names:?}");
    }
    // The one place $PLUGINSDIR and $TEMP may appear: the folder's macro setting them, which
    // writes them and reads nothing from them.
    let sets = [
        "UnsafeStrCpy $PLUGINSDIR \"$0\"",
        "UnsafeStrCpy $TEMP \"$0\"",
    ];
    // LogicLib's words, which expand to plain jumps (FileExists to IfFileExists); anything else
    // in ${...} is a define.
    let logic = [
        "If",
        "IfNot",
        "ElseIf",
        "Else",
        "EndIf",
        "AndIf",
        "OrIf",
        "Do",
        "Loop",
        "ExitDo",
        "Errors",
        "FileExists",
    ];
    for (name, body) in &macros {
        for line in *body {
            assert!(!line.starts_with("Call "), "{name}: {line}");
            if name == "CLAUSEO_SAFE_PLUGINSDIR" && sets.contains(&line.as_str()) {
                continue;
            }
            if let Some(word) = refused_word(line) {
                panic!("{name}: {word} in {line}");
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
            "FindWindow $R5 \"${CLAUSEO_MAIN_WINDOW_CLASS}\"",
            "${If} $R5 <> 0",
            "SetErrorLevel ${CLAUSEO_ADMIN_APP_RUNNING}",
            "Quit",
            "${EndIf}",
            "!insertmacro CLAUSEO_SAFE_PLUGINSDIR",
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
            "${ElseIf} $R1 = 3",
            "IntOp $R8 ${CLAUSEO_ADMIN_EXIT_BASE} + ${CLAUSEO_ADOPT_FAILED}",
            "${Else}",
            "!insertmacro CLAUSEO_ADMIN_COPY '--owner-sid $R2 --user-install-dir \"$INSTDIR\" \
             --data-dir \"$R3\"'",
            "${If} $R8 <> ${CLAUSEO_ADMIN_NOT_RUN}",
            "${AndIf} $R8 <> ${CLAUSEO_ADMIN_REMOVED}",
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

#[test]
fn the_main_window_class_is_clauseos_own_and_the_installer_looks_for_it() {
    // Unique to Clauseo (tao's default, `Tauri Window`, is every Tauri app's).
    assert_eq!(crate::MAIN_WINDOW_CLASS, "ClauseoMainWindow");
    let code = admin_code();
    line_with(
        &code,
        &[&format!(
            "!define CLAUSEO_MAIN_WINDOW_CLASS \"{}\"",
            crate::MAIN_WINDOW_CLASS
        )],
    );
    // Set where the main window is built.
    let lib = read("src/lib.rs");
    assert!(
        lib.contains(".window_classname(MAIN_WINDOW_CLASS)"),
        "lib.rs"
    );
    // The e2e close finds the main window by the same class.
    let close = read("../../ui/e2e/app-windows/close.ts");
    assert!(
        close.contains(&format!(
            "export const WINDOW_CLASS = \"{}\";",
            crate::MAIN_WINDOW_CLASS
        )),
        "close.ts"
    );
    // Both instances look before anything happens: the outer one before it asks for the prompt.
    let ask = macro_body(&code, "CLAUSEO_ASK_FOR_THE_ADMIN_SECTION");
    let find = line_with(ask, &["FindWindow $R5 \"${CLAUSEO_MAIN_WINDOW_CLASS}\""]);
    assert!(find < line_with(ask, &["$(adminCopyAsk)"]), "{ask:?}");
    assert_run(
        ask,
        &[
            "${ElseIf} $R6 = 6",
            "DetailPrint \"$(adminCopyAppRunning)\"",
            "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(adminCopyAppRunning)\"",
            "SetErrorLevel ${CLAUSEO_EXIT_APP_RUNNING}",
        ],
    );
    line_with(&code, &["!define CLAUSEO_EXIT_APP_RUNNING 24"]);
    line_with(&code, &["!define CLAUSEO_ADMIN_APP_RUNNING 97"]);
}
