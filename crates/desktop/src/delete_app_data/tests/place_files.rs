//! No file Section Install places is ever skipped: release run 44026502738's arm64 smoke exited 0
//! over an x64 install and kept the x64 `Clauseo.exe`, since NSIS's default skips a file it can't
//! open in a silent install and only sets the error flag. The template's blocks (`place-file`,
//! `place-preflight`, `place-main-exe`, `place-resource`, `place-binary`, `place-files-end`)
//! first check every target already in the folder free (a held one stops the install before
//! anything is replaced), then set `SetOverwrite try`, under which NSIS never shows its own box,
//! and wrap every `File` in a loop. Both try three times, 2 s apart, then say `clauseoFileInUse`
//! (no box under `/S`), set exit code 29 and abort before the uninstaller and the registry are
//! written; `nsis_run` runs the template's lines on a fake machine.

use std::collections::HashMap;

use super::admin_copy::macro_body;
use super::nsis_run::{Machine, defines, parse_all, preprocess};
use super::sources::{hook_code, read};
use super::template::{TEMPLATE, block_code, split_blocks};

const EXE: &str = r"C:\Users\a\AppData\Local\Clauseo\Clauseo.exe";

/// The `place-file` block's code lines, as owned strings for `macro_body`.
pub(super) fn place_code() -> Vec<String> {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    block_code(&blocks, "place-file")
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The lines of the macro `name` for `EXE`, `CLAUSEO_PLACE_FAILED` inserted in place (its
/// parameter has the same name, so the same define stands for both).
pub(super) fn place_lines(
    code: &[String],
    name: &str,
    defines: &HashMap<String, String>,
) -> Vec<String> {
    let restore = preprocess(&macro_body(code, "CLAUSEO_PLACE_RESTORE")[1..], defines);
    let failed: Vec<String> = preprocess(&macro_body(code, "CLAUSEO_PLACE_FAILED")[1..], defines)
        .into_iter()
        .flat_map(|line| {
            if line == "!insertmacro CLAUSEO_PLACE_RESTORE" {
                restore.clone()
            } else {
                vec![line]
            }
        })
        .collect();
    preprocess(&macro_body(code, name)[1..], defines)
        .into_iter()
        .flat_map(|line| {
            if line == "!insertmacro CLAUSEO_PLACE_FAILED \"${_CLAUSEO_FILE}\"" {
                failed.clone()
            } else {
                vec![line]
            }
        })
        .collect()
}

/// `lines` built from the `place-file` macros by `build`, then a line standing for the rest of
/// the section (the uninstaller and the registry), run with the first `held` opens failing and
/// `EXE` there or not: the machine it leaves.
fn run_with(
    build: impl Fn(&[String], &HashMap<String, String>) -> Vec<String>,
    held: usize,
    silent: bool,
    exe_there: bool,
    vars: &[(&str, &str)],
) -> Machine {
    let code = place_code();
    let mut defines: HashMap<String, String> = defines(&code);
    defines.insert("_CLAUSEO_FILE".to_owned(), EXE.to_owned());
    let mut lines = build(&code, &defines);
    lines.push("DetailPrint rest-of-the-section".to_owned());
    let mut machine = Machine::new(defines, None);
    machine.silent = silent;
    machine.held = held;
    for (var, value) in vars {
        machine.set_var(var, value);
    }
    if exe_there {
        machine.files.push(EXE.to_owned());
    }
    machine.run(&parse_all(&lines));
    assert!(machine.stack.is_empty(), "{:?}", machine.stack);
    machine
}

/// `CLAUSEO_PLACE_BEGIN`, a `File`, then `CLAUSEO_PLACE_END` for `EXE`, the way the blocks
/// insert them, with the first `held` tries failing.
fn run_place(held: usize, silent: bool) -> Machine {
    run_with(
        |code, defines| {
            let mut lines = place_lines(code, "CLAUSEO_PLACE_BEGIN", defines);
            lines.push("File \"Clauseo.exe\"".to_owned());
            lines.extend(place_lines(code, "CLAUSEO_PLACE_END", defines));
            lines
        },
        held,
        silent,
        true,
        &[],
    )
}

/// The preflight's `CLAUSEO_PLACE_FREE` for `EXE`, with the first `held` opens failing.
fn run_free(held: usize, silent: bool, exe_there: bool) -> Machine {
    run_with(
        |code, defines| place_lines(code, "CLAUSEO_PLACE_FREE", defines),
        held,
        silent,
        exe_there,
        &[],
    )
}

/// The reinstall page's `CLAUSEO_PLACE_FREE` (`$ClauseoPlacePage` 1) for `EXE`, interactive
/// or passive, with every open failing.
fn run_free_on_page(passive: bool) -> Machine {
    run_with(
        |code, defines| place_lines(code, "CLAUSEO_PLACE_FREE", defines),
        3,
        false,
        true,
        &[
            ("$ClauseoPlacePage", "1"),
            ("$PassiveMode", if passive { "1" } else { "0" }),
        ],
    )
}

/// The failure's lines after the last try, for `EXE`; `box` unless /S.
fn failed(shows_box: bool) -> Vec<String> {
    let mut said = vec![
        "DetailPrint $(clauseoFileInUse)".to_owned(),
        format!("DetailPrint {EXE}"),
    ];
    if shows_box {
        said.push("MessageBox $(clauseoFileInUse)".to_owned());
    }
    said.extend(["SetErrorLevel 29".to_owned(), "Abort".to_owned()]);
    said
}

#[test]
fn the_blocks_place_every_file_through_the_loop_with_no_nsis_box() {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    let exe = "\"$INSTDIR\\${MAINBINARYNAME}.exe\"";
    assert_eq!(
        block_code(&blocks, "place-main-exe"),
        [
            "SetOverwrite try",
            "AllowSkipFiles off",
            &format!("!insertmacro CLAUSEO_PLACE_BEGIN {exe}"),
            "File \"${MAINBINARYSRCPATH}\"",
            &format!("!insertmacro CLAUSEO_PLACE_END {exe}"),
        ]
    );
    // `\\` before a template mark is one backslash once rendered (as in the template's own
    // CreateDirectory and Delete lines).
    assert_eq!(
        block_code(&blocks, "place-resource"),
        [
            "!insertmacro CLAUSEO_PLACE_BEGIN \"$INSTDIR\\\\{{this.[1]}}\"",
            "File /a \"/oname={{this.[1]}}\" \"{{no-escape @key}}\"",
            "!insertmacro CLAUSEO_PLACE_END \"$INSTDIR\\\\{{this.[1]}}\"",
        ]
    );
    assert_eq!(
        block_code(&blocks, "place-binary"),
        [
            "!insertmacro CLAUSEO_PLACE_BEGIN \"$INSTDIR\\\\{{this}}\"",
            "File /a \"/oname={{this}}\" \"{{no-escape @key}}\"",
            "!insertmacro CLAUSEO_PLACE_END \"$INSTDIR\\\\{{this}}\"",
        ]
    );
    assert_eq!(block_code(&blocks, "place-files-end"), ["SetOverwrite on"]);
}

#[test]
fn every_file_in_section_install_is_in_the_loop_and_nothing_turns_skipping_back_on() {
    let text = read(TEMPLATE);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let start = lines
        .iter()
        .position(|line| *line == "Section Install")
        .unwrap();
    let end = start
        + lines[start..]
            .iter()
            .position(|line| *line == "SectionEnd")
            .unwrap();
    let section = &lines[start..end];
    let files: Vec<usize> = (0..section.len())
        .filter(|at| section[*at].starts_with("File "))
        .collect();
    assert_eq!(files.len(), 3, "{section:?}");
    for at in files {
        let begin = section[at - 1]
            .strip_prefix("!insertmacro CLAUSEO_PLACE_BEGIN ")
            .unwrap_or_else(|| panic!("{}", section[at]));
        let end = section[at + 1]
            .strip_prefix("!insertmacro CLAUSEO_PLACE_END ")
            .unwrap_or_else(|| panic!("{}", section[at]));
        assert_eq!(begin, end, "{}", section[at]);
    }
    // SetOverwrite try before the first File, and on again only after the last.
    let at = |wanted: &str| section.iter().position(|line| *line == wanted).unwrap();
    assert!(at("SetOverwrite try") < at("File \"${MAINBINARYSRCPATH}\""));
    assert!(at("SetOverwrite on") > at("File /a \"/oname={{this}}\" \"{{no-escape @key}}\""));
    // Nowhere in the template or the hooks is skipping allowed again.
    let mut code: Vec<String> = text.lines().map(|line| line.trim().to_owned()).collect();
    code.extend(hook_code());
    for line in &code {
        assert_ne!(line, "AllowSkipFiles on");
    }
}

#[test]
fn a_file_placed_at_once_or_after_a_passing_lock_lets_the_install_go_on() {
    for silent in [false, true] {
        let machine = run_place(0, silent);
        assert!(!machine.aborted);
        assert_eq!(
            machine.said,
            ["File Clauseo.exe", "DetailPrint rest-of-the-section"]
        );
        let machine = run_place(2, silent);
        assert!(!machine.aborted);
        assert_eq!(
            machine.said,
            [
                "File Clauseo.exe",
                "Sleep 2000",
                "File Clauseo.exe",
                "Sleep 2000",
                "File Clauseo.exe",
                "DetailPrint rest-of-the-section",
            ]
        );
    }
}

#[test]
fn a_file_still_held_after_three_tries_shows_the_box_sets_29_and_aborts() {
    let tries: Vec<String> = [
        "File Clauseo.exe",
        "Sleep 2000",
        "File Clauseo.exe",
        "Sleep 2000",
        "File Clauseo.exe",
    ]
    .map(str::to_owned)
    .to_vec();
    // Interactive and /P (the updater's run): the box, in Clauseo's words; /S: none.
    for silent in [false, true] {
        let machine = run_place(3, silent);
        assert!(machine.aborted);
        let mut said = tries.clone();
        said.extend(failed(!silent));
        assert_eq!(machine.said, said, "silent {silent}");
    }
}

#[test]
fn the_preflight_opens_a_file_that_is_there_and_closes_it_or_skips_one_that_isnt() {
    for silent in [false, true] {
        let machine = run_free(0, silent, true);
        assert!(!machine.aborted);
        assert_eq!(
            machine.said,
            [
                format!("FileOpen {EXE}"),
                "DetailPrint rest-of-the-section".to_owned()
            ]
        );
        // "a" would create a missing file: a missing one isn't opened at all.
        let machine = run_free(0, silent, false);
        assert!(!machine.aborted);
        assert_eq!(machine.said, ["DetailPrint rest-of-the-section"]);
        // A lock that passes within the tries.
        let machine = run_free(2, silent, true);
        assert!(!machine.aborted);
        assert_eq!(
            machine.said,
            [
                format!("FileOpen {EXE}"),
                "Sleep 2000".to_owned(),
                format!("FileOpen {EXE}"),
                "Sleep 2000".to_owned(),
                format!("FileOpen {EXE}"),
                "DetailPrint rest-of-the-section".to_owned(),
            ]
        );
    }
}

#[test]
fn a_file_the_preflight_finds_held_after_three_tries_stops_the_install_with_29() {
    for silent in [false, true] {
        let machine = run_free(3, silent, true);
        assert!(machine.aborted);
        let mut said = vec![
            format!("FileOpen {EXE}"),
            "Sleep 2000".to_owned(),
            format!("FileOpen {EXE}"),
            "Sleep 2000".to_owned(),
            format!("FileOpen {EXE}"),
        ];
        said.extend(failed(!silent));
        assert_eq!(machine.said, said, "silent {silent}");
    }
    // The open is "a" (read and write, the file kept, OPEN_ALWAYS), closed at once, never "w".
    let code = place_code();
    let free = macro_body(&code, "CLAUSEO_PLACE_FREE");
    assert!(free.contains(&"FileOpen $ClauseoPlaceHandle \"${_CLAUSEO_FILE}\" a".to_owned()));
    assert!(free.contains(&"FileClose $ClauseoPlaceHandle".to_owned()));
    assert!(
        free.iter()
            .all(|line| !line.starts_with("FileWrite") && !line.starts_with("FileSeek"))
    );
}

/// Each `!insertmacro <name> <path>` in Section Install, with the `{{#each ...}}` list it stands
/// in (`""` outside any), in order.
fn inserted(section: &[&str], name: &str) -> Vec<(String, String)> {
    let mut list = String::new();
    let mut out = Vec::new();
    for line in section {
        if let Some(each) = line.strip_prefix("{{#each ") {
            list = each.trim_end_matches("}}").to_owned();
        } else if *line == "{{/each}}" {
            list.clear();
        } else if let Some(path) = line.strip_prefix(&format!("!insertmacro {name} ")) {
            out.push((list.clone(), path.to_owned()));
        }
    }
    out
}

#[test]
fn the_preflight_checks_every_target_the_place_loop_writes_before_the_first_file() {
    let text = read(TEMPLATE);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let start = lines
        .iter()
        .position(|line| *line == "Section Install")
        .unwrap();
    let end = start
        + lines[start..]
            .iter()
            .position(|line| *line == "SectionEnd")
            .unwrap();
    let section = &lines[start..end];
    // The same targets from the same lists: the main exe, each resource, each sidecar.
    let free = inserted(section, "CLAUSEO_PLACE_FREE");
    let placed = inserted(section, "CLAUSEO_PLACE_BEGIN");
    assert_eq!(free, placed);
    let lists: Vec<&str> = free.iter().map(|(list, _)| list.as_str()).collect();
    assert_eq!(lists, ["", "resources", "binaries"]);
    // After Tauri's app-running check, and every check before the first File.
    let at = |pred: &dyn Fn(&str) -> bool| section.iter().position(|line| pred(line)).unwrap();
    let check = at(&|line| line.starts_with("!insertmacro CheckIfAppIsRunning "));
    let first_free = at(&|line| line.starts_with("!insertmacro CLAUSEO_PLACE_FREE "));
    let last_free = section
        .iter()
        .rposition(|line| line.starts_with("!insertmacro CLAUSEO_PLACE_FREE "))
        .unwrap();
    let first_file = at(&|line| line.starts_with("File "));
    assert!(
        check < first_free && last_free < first_file,
        "{check} {first_free} {last_free} {first_file}"
    );
    let (blocks, _) = split_blocks(&text);
    assert_eq!(
        block_code(&blocks, "place-preflight"),
        [
            "StrCpy $ClauseoPlacePage 0",
            "StrCpy $ClauseoPlaceCleared 0",
            "!insertmacro CLAUSEO_PLACE_FREE \"$INSTDIR\\${MAINBINARYNAME}.exe\"",
            "{{#each resources}}",
            "!insertmacro CLAUSEO_PLACE_FREE \"$INSTDIR\\\\{{this.[1]}}\"",
            "{{/each}}",
            "{{#each binaries}}",
            "!insertmacro CLAUSEO_PLACE_FREE \"$INSTDIR\\\\{{this}}\"",
            "{{/each}}",
            "!insertmacro CLAUSEO_PLACE_FREED",
        ]
    );
}

#[test]
fn the_reinstall_page_checks_the_same_files_before_either_old_uninstaller_runs() {
    let text = read(TEMPLATE);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let start = lines
        .iter()
        .position(|line| *line == "Function PageLeaveReinstall")
        .unwrap();
    let end = start
        + lines[start..]
            .iter()
            .position(|line| *line == "FunctionEnd")
            .unwrap();
    let leave = &lines[start..end];
    let section_start = lines
        .iter()
        .position(|line| *line == "Section Install")
        .unwrap();
    let section_end = section_start
        + lines[section_start..]
            .iter()
            .position(|line| *line == "SectionEnd")
            .unwrap();
    // The same targets, lists and order as the place loop.
    let placed = inserted(&lines[section_start..section_end], "CLAUSEO_PLACE_BEGIN");
    assert_eq!(inserted(leave, "CLAUSEO_PLACE_FREE"), placed);
    // Right at reinst_uninstall:, every check before both old uninstallers' ExecWait (WiX's and
    // NSIS's), the page flag set around them.
    let at = |wanted: &str| leave.iter().position(|line| *line == wanted).unwrap();
    let label = at("reinst_uninstall:");
    assert_eq!(leave[label + 1], "; >>> clauseo: reinstall-preflight");
    let last_free = leave
        .iter()
        .rposition(|line| line.starts_with("!insertmacro CLAUSEO_PLACE_FREE "))
        .unwrap();
    let execs: Vec<usize> = (0..leave.len())
        .filter(|at| leave[*at] == "ExecWait '$R1' $0")
        .collect();
    assert_eq!(execs.len(), 2, "{leave:?}");
    assert!(execs.iter().all(|exec| last_free < *exec && label < *exec));
    let (blocks, _) = split_blocks(&text);
    assert_eq!(
        block_code(&blocks, "reinstall-preflight"),
        [
            "StrCpy $ClauseoPlacePage 1",
            "StrCpy $ClauseoPlaceCleared 0",
            "!insertmacro CLAUSEO_PLACE_FREE \"$INSTDIR\\${MAINBINARYNAME}.exe\"",
            "{{#each resources}}",
            "!insertmacro CLAUSEO_PLACE_FREE \"$INSTDIR\\\\{{this.[1]}}\"",
            "{{/each}}",
            "{{#each binaries}}",
            "!insertmacro CLAUSEO_PLACE_FREE \"$INSTDIR\\\\{{this}}\"",
            "{{/each}}",
            "!insertmacro CLAUSEO_PLACE_FREED",
            "StrCpy $ClauseoPlacePage 0",
        ]
    );
    // The macros are defined before the page uses them (NSIS reads the script in order).
    let defined = lines
        .iter()
        .position(|line| line.starts_with("!macro CLAUSEO_PLACE_FREE"))
        .unwrap();
    assert!(defined < start);
}

#[test]
fn a_held_file_on_the_reinstall_page_goes_back_to_it_or_quits_with_29_when_passive() {
    let tries = [
        format!("FileOpen {EXE}"),
        "Sleep 2000".to_owned(),
        format!("FileOpen {EXE}"),
        "Sleep 2000".to_owned(),
        format!("FileOpen {EXE}"),
        "DetailPrint $(clauseoFileInUse)".to_owned(),
        format!("DetailPrint {EXE}"),
        "MessageBox $(clauseoFileInUse)".to_owned(),
    ];
    // Interactive: the box, then Abort keeps the page; no exit code, as the lawyer may choose
    // again and install.
    let machine = run_free_on_page(false);
    assert!(machine.aborted);
    let mut said = tries.to_vec();
    said.push("Abort".to_owned());
    assert_eq!(machine.said, said);
    // Passive: 29, and Quit before the old uninstaller.
    let machine = run_free_on_page(true);
    let mut said = tries.to_vec();
    said.extend(["SetErrorLevel 29".to_owned(), "Quit".to_owned()]);
    // Quit ends the installer; the fake doesn't stop at Quit and reaches the Abort after it.
    said.push("Abort".to_owned());
    assert_eq!(machine.said, said);
}

#[test]
fn the_loop_says_so_in_front_of_every_window_with_no_box_under_s() {
    let code = place_code();
    let end = macro_body(&code, "CLAUSEO_PLACE_FAILED");
    assert!(
        end.contains(
            &"MessageBox MB_OK|MB_ICONSTOP|MB_SETFOREGROUND|MB_TOPMOST \"$(clauseoFileInUse)\" \
              /SD IDOK"
                .to_owned()
        ),
        "{end:?}"
    );
    let defines = defines(&code);
    assert_eq!(defines["CLAUSEO_PLACE_TRIES"], "3");
    assert_eq!(defines["CLAUSEO_PLACE_WAIT_MS"], "2000");
    assert_eq!(defines["CLAUSEO_EXIT_FILE_IN_USE"], "29");
}

#[test]
fn nothing_records_the_new_version_before_every_file_is_placed() {
    // In Section Install, the uninstaller and every registry write come after the last File.
    let text = read(TEMPLATE);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let start = lines
        .iter()
        .position(|line| *line == "Section Install")
        .unwrap();
    let last_file = start
        + lines[start..]
            .iter()
            .position(|line| {
                line.starts_with("!insertmacro CLAUSEO_PLACE_END \"$INSTDIR\\\\{{this}}\"")
            })
            .unwrap();
    let first_write = start
        + lines[start..]
            .iter()
            .position(|line| line.starts_with("WriteReg") || line.starts_with("WriteUninstaller"))
            .unwrap();
    assert!(last_file < first_write, "{last_file} {first_write}");
    assert!(
        lines[first_write..]
            .iter()
            .any(|line| line.contains("\"DisplayVersion\" \"${VERSION}\""))
    );
    // Nothing before it in the section writes the registry either: the pre-install hook and
    // the hooks' macros it inserts (the others are Tauri's Restart Manager ones).
    let code = hook_code();
    let preinstall = macro_body(&code, "NSIS_HOOK_PREINSTALL");
    let mut bodies = vec![("NSIS_HOOK_PREINSTALL", preinstall)];
    for line in preinstall {
        if let Some(name) = line.strip_prefix("!insertmacro ") {
            let name = name.split_whitespace().next().unwrap();
            assert_eq!(name, "CLAUSEO_REPAIR_WAIT", "{preinstall:?}");
            bodies.push((name, macro_body(&code, name)));
        }
    }
    for (name, body) in bodies {
        for line in body {
            assert!(!line.starts_with("WriteReg"), "{name}: {line}");
            if let Some(inserted) = line.strip_prefix("!insertmacro ") {
                assert!(
                    inserted.starts_with("RestartManager_") || name == "NSIS_HOOK_PREINSTALL",
                    "{name}: {line}"
                );
            }
        }
    }
}

#[test]
fn exit_code_29_is_free_and_listed_for_firm_it() {
    // NSIS's own, what Intune and SCCM read as success, a reboot or a retry; distinct from the
    // hooks' codes (admin_copy_ask.rs, the_installers_exit_codes_are_their_own).
    assert!(![0, 1, 2, 1223, 1618, 1641, 1707, 3010].contains(&29));
    let firm_it = read("../../docs/firm-it.md");
    let row = "| 29 | Install: stopped, because a file in the install folder that it had to \
               replace, such as `Clauseo.exe`, stayed open or locked by another program through \
               three tries over about 4 seconds; Programs and Features keeps the version that \
               was there. Restart the computer, or close what holds the file, then install again |";
    // Both install tables in section 1 (/S and interactive), and section 10's.
    assert_eq!(firm_it.lines().filter(|line| *line == row).count(), 3);
}

#[test]
fn the_words_are_clauseos_in_both_languages_and_listed_for_review() {
    let text = |file: &str, constant: &str| {
        read(file)
            .lines()
            .find_map(|line| line.strip_prefix(&format!("LangString clauseoFileInUse {constant} ")))
            .unwrap_or_else(|| panic!("{file}: no clauseoFileInUse"))
            .to_owned()
    };
    assert_eq!(
        text("windows/nsis/English.nsh", "${LANG_ENGLISH}"),
        "\"Clauseo couldn't finish installing, because another program has one of its files open. \
         Restart this computer, then try again.\""
    );
    let hindi = text("windows/nsis/Hindi.nsh", "${LANG_HINDI}");
    assert!(hindi.contains("Clauseo"), "{hindi}");
    // "This computer", and only "open": no "locked" (the app track's settled meaning).
    assert!(
        hindi.contains("इस कंप्यूटर") && !hindi.contains("लॉक"),
        "{hindi}"
    );
    assert!(!hindi.contains('$') && !hindi.contains('\\'), "{hindi}");
    let approvals: serde_json::Value =
        serde_json::from_str(&read("../../ui/app/doc-approvals.json")).unwrap();
    let lines = &approvals["crates/desktop/windows/nsis/English.nsh"]["lines"];
    assert!(
        lines
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line == "clauseoFileInUse")
    );
}
