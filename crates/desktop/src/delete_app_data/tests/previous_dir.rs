//! The installer acts on the previous install folder only when `hooks.nsh`'s `CLAUSEO_PREVIOUS_DIR_OK` accepts it: an absolute path on a fixed drive,
//! equal to the default folder or the uninstall key's `InstallLocation`, holding a regular
//! `uninstall.exe`, and per machine with a chain only administrators can change. The template's
//! The folder is the vendor key's (`SHCTX\${MANUPRODUCTKEY}`, default value) per user; per machine
//! it is the HKLM uninstall key's `InstallLocation`, else the default, and a vendor key that
//! differs is ignored and said. The template's blocks `reinstall-guard` and `restore-location-guard` run the check right before each use: a
//! refused folder starts no old uninstaller and leaves `$INSTDIR` at the default, with one details
//! line. The native calls (drive type, attributes, the chain) are faked by path here; the Windows
//! VM runs them for real.

use std::collections::HashMap;

use super::nsis_run::{Machine, Node, Value, defines, parse_all, preprocess};
use super::sources::{hook_code, line_with, read};
use super::template::{TEMPLATE, block_named, split_blocks};

const PROGRAM_FILES: &str = r"C:\Program Files";
const LOCAL_APP_DATA: &str = r"C:\Users\me\AppData\Local";
const MACHINE_DEFAULT: &str = r"C:\Program Files\Clauseo";
const USER_DEFAULT: &str = r"C:\Users\me\AppData\Local\Clauseo";
const UNINSTKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo";
const MANUPRODUCTKEY: &str = r"Software\Clauseo\Clauseo";
const OTHER: &str = r"D:\Apps\Clauseo";
/// What `UninstallString` names unless a test says otherwise: a program that isn't the folder's.
const OTHER_PROGRAM: &str = r#""C:\Users\other\other.exe""#;
/// `FILE_ATTRIBUTE_ARCHIVE`, a plain file; a folder; a reparse point (a link to a file).
const FILE: i64 = 0x20;
const FOLDER: i64 = 0x10;
const LINK: i64 = 0x420;

/// What the fake machine holds: the install mode, the registry's previous folder and
/// `InstallLocation` (as written, quotes and all), drives that aren't fixed, the attributes of
/// each `uninstall.exe` there is, and the `uninstall.exe` paths whose chain someone else can change.
#[derive(Clone)]
struct World {
    mode: &'static str,
    previous: Option<&'static str>,
    install_location: Option<String>,
    /// The uninstall key's `UninstallString`.
    uninstall_string: &'static str,
    not_fixed: Vec<&'static str>,
    uninstallers: Vec<(String, i64)>,
    open_chains: Vec<String>,
}

impl World {
    /// A per-`mode` machine whose previous folder is `previous`, with a regular `uninstall.exe` in
    /// it, a fixed drive and a closed chain, and no `InstallLocation`.
    fn new(mode: &'static str, previous: &'static str) -> Self {
        Self {
            mode,
            previous: Some(previous),
            install_location: None,
            uninstall_string: OTHER_PROGRAM,
            not_fixed: Vec::new(),
            uninstallers: vec![(exe(previous), FILE)],
            open_chains: Vec::new(),
        }
    }

    fn located(mut self, folder: &str) -> Self {
        self.install_location = Some(format!("\"{folder}\""));
        self
    }

    fn default(&self) -> &'static str {
        if self.mode == "perMachine" {
            MACHINE_DEFAULT
        } else {
            USER_DEFAULT
        }
    }
}

fn exe(folder: &str) -> String {
    format!(r"{}\uninstall.exe", folder.trim_end_matches('\\'))
}

/// The code lines of `name` in `code`, `!macro` and `!macroend` left out.
fn macro_body<'a>(code: &'a [String], name: &str) -> &'a [String] {
    let start = code
        .iter()
        .position(|line| line == &format!("!macro {name}"))
        .unwrap_or_else(|| panic!("no macro {name}"));
    let end = start + line_with(&code[start..], &["!macroend"]);
    &code[start + 1..end]
}

/// A fake for a native call: `out` is `default`, or the value of the row whose key `input` equals.
fn table(out: &str, default: &str, input: &str, rows: &[(String, String)]) -> Vec<Node> {
    let mut lines = vec![format!("StrCpy {out} {default}")];
    for (key, value) in rows {
        lines.push(format!("${{If}} {input} == \"{key}\""));
        lines.push(format!("StrCpy {out} {value}"));
        lines.push("${EndIf}".to_owned());
    }
    parse_all(&lines)
}

fn mode_defines(mode: &str) -> HashMap<String, String> {
    let mut defines = defines(&hook_code());
    for (name, value) in [
        ("INSTALLMODE", mode),
        ("PRODUCTNAME", "Clauseo"),
        ("UNINSTKEY", UNINSTKEY),
        ("MANUPRODUCTKEY", MANUPRODUCTKEY),
        ("NSIS_PTR_SIZE", "4"),
        ("ARCH", "x64"),
    ] {
        defines.insert(name.to_owned(), value.to_owned());
    }
    defines
}

/// Sets the fakes from `world`: the registry, and the native calls' answers.
fn load(machine: &mut Machine, world: &World) {
    let previous = world.previous.map(|p| vec![("", Value::Text(p))]);
    machine.keys.insert(
        format!(r"SHCTX\{MANUPRODUCTKEY}"),
        previous.unwrap_or_default(),
    );
    let mut uninstall = vec![("UninstallString", Value::Text(world.uninstall_string))];
    if let Some(location) = world.install_location.clone() {
        uninstall.push(("InstallLocation", Value::Text(String::leak(location))));
    }
    machine
        .keys
        .insert(format!(r"SHCTX\{UNINSTKEY}"), uninstall);
    let drives: Vec<(String, String)> = world
        .not_fixed
        .iter()
        .map(|root| ((*root).to_owned(), "2".to_owned()))
        .collect();
    let files: Vec<(String, String)> = world
        .uninstallers
        .iter()
        .map(|(path, attributes)| (path.clone(), attributes.to_string()))
        .collect();
    let chains: Vec<(String, String)> = world
        .open_chains
        .iter()
        .map(|path| (path.clone(), "0".to_owned()))
        .collect();
    let fakes = [
        ("CLAUSEO_DRIVE_TYPE", table("$2", "3", "$1", &drives)),
        ("CLAUSEO_FILE_ATTRIBUTES", table("$2", "-1", "$1", &files)),
        ("CLAUSEO_CHAIN_SAFE", table("$5", "1", "$0", &chains)),
    ];
    for (name, nodes) in fakes {
        machine.inline.insert(name.to_owned(), nodes);
    }
}

/// A machine for `world`, the guard's macros real, `$INSTDIR` the template's default, and every
/// register but `$4` seeded so a test can see it kept.
fn machine(world: &World) -> Machine {
    let defines = mode_defines(world.mode);
    let code = hook_code();
    let mut machine = Machine::new(defines.clone(), None);
    for name in [
        "CLAUSEO_INSTALL_LOCATION",
        "CLAUSEO_PREVIOUS_DIR_OK",
        "CLAUSEO_PREVIOUS_DIR_GUARD",
        "CLAUSEO_PREVIOUS_UNINSTALLER",
        "NSIS_HOOK_PREINSTALL",
    ] {
        let body = preprocess(macro_body(&code, name), &defines);
        machine.inline.insert(name.to_owned(), parse_all(&body));
    }
    load(&mut machine, world);
    machine.set_var("$PROGRAMFILES64", PROGRAM_FILES);
    machine.set_var("$LOCALAPPDATA", LOCAL_APP_DATA);
    machine.set_var("$INSTDIR", world.default());
    machine.set_var("$ClauseoPreviousIgnored", "0");
    machine.set_var("$ClauseoPreviousKeyMismatch", "0");
    for register in registers() {
        machine.set_var(&register, &format!("kept {register}"));
    }
    machine
}

fn registers() -> Vec<String> {
    (0..10)
        .filter(|n| *n != 4)
        .flat_map(|n| [format!("${n}"), format!("$R{n}")])
        .chain(std::iter::once("$R4".to_owned()))
        .collect()
}

/// The template's lines of `function`, trimmed, from its first line to `FunctionEnd`.
fn function(name: &str) -> Vec<String> {
    let text = read(TEMPLATE);
    let lines: Vec<String> = text.lines().map(|l| l.trim().to_owned()).collect();
    let start = lines
        .iter()
        .position(|l| *l == format!("Function {name}"))
        .unwrap();
    let end = start
        + lines[start..]
            .iter()
            .position(|l| l == "FunctionEnd")
            .unwrap();
    lines[start..=end].to_vec()
}

/// The code of the template's block `name`, under `mode`'s defines.
fn block(name: &str, mode: &str) -> Vec<Node> {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    let code: Vec<String> = block_named(&blocks, name)
        .lines
        .iter()
        .filter(|l| !l.is_empty() && !l.starts_with(';'))
        .cloned()
        .collect();
    parse_all(&preprocess(&code, &mode_defines(mode)))
}

const READ_PREVIOUS: &str = "ReadRegStr $4 SHCTX \"${MANUPRODUCTKEY}\" \"\"";
const APPEND: &str = "StrCpy $R1 \"$R1 _?=$4\"";
const EXEC: &str = "ExecWait '$R1' $0";

/// `RestorePreviousInstallLocation` as the template runs it: the read, the block, then
/// `StrCmp $4 "" +2 0` / `StrCpy $INSTDIR $4` (a jump the interpreter doesn't take, so run here).
fn restore(machine: &mut Machine, mode: &str) {
    let lines = function("RestorePreviousInstallLocation");
    let read_at = line_with(&lines, &[READ_PREVIOUS]);
    assert_eq!(lines[read_at + 1], "; >>> clauseo: restore-location-guard");
    let after = lines
        .iter()
        .position(|l| l == "; <<< clauseo: restore-location-guard");
    let after = after.unwrap();
    assert_eq!(
        lines[after + 1..after + 3],
        ["StrCmp $4 \"\" +2 0", "StrCpy $INSTDIR $4"]
    );
    machine.run(&parse_all(&[READ_PREVIOUS.to_owned()]));
    machine.run(&block("restore-location-guard", mode));
    if !machine.vars["$4"].is_empty() {
        machine.run(&parse_all(&["StrCpy $INSTDIR $4".to_owned()]));
    }
}

/// The reinstall page's uninstall, from the read of the previous folder to the `ExecWait`: the
/// read, the block (the guard, then the command into $R1), then (unless it jumped to
/// `reinst_done`) the template's own appends and start. `/UPDATE` and `/P` aren't set here,
/// so their two `${IfThen}` lines append nothing and are only checked to be there.
fn reinstall(machine: &mut Machine, mode: &str) {
    let lines = function("PageLeaveReinstall");
    let read_at = line_with(&lines, &[READ_PREVIOUS]);
    assert_eq!(lines[read_at + 1], "; >>> clauseo: reinstall-guard");
    machine.run(&parse_all(&[READ_PREVIOUS.to_owned()]));
    machine.set_var("$R1", "");
    machine.run(&block("reinstall-guard", mode));
    if !machine.said.iter().any(|s| s == "Goto reinst_done") {
        let end = line_with(&lines, &["; <<< clauseo: reinstall-guard"]);
        assert!(lines[end + 1].starts_with("${IfThen} $UpdateMode = 1 "));
        assert!(lines[end + 2].starts_with("${IfThen} $PassiveMode = 1 "));
        assert!(lines[end + 3].starts_with(APPEND), "{}", lines[end + 3]);
        assert_eq!(lines[end + 4], EXEC);
        machine.run(&parse_all(&[APPEND.to_owned(), EXEC.to_owned()]));
    }
}

/// What a run did: the old uninstaller's command (if started), `$INSTDIR` after the restore, and
/// the install's details lines.
#[derive(Debug, PartialEq)]
struct Outcome {
    started: Option<String>,
    instdir: String,
    told: bool,
    mismatch: bool,
}

fn run(world: &World) -> Outcome {
    let mut machine = machine(world);
    restore(&mut machine, world.mode);
    let instdir = machine.vars["$INSTDIR"].clone();
    machine.said.clear();
    reinstall(&mut machine, world.mode);
    let started = machine
        .said
        .iter()
        .find_map(|s| s.strip_prefix("ExecWait ").map(str::to_owned));
    if started.is_none() {
        // Skipped: nothing to uninstall, past the checks of $0, and the window back.
        assert_eq!(machine.said, ["Goto reinst_done"]);
        assert_eq!(machine.vars["$0"], "0");
    }
    assert_eq!(
        machine.vars["$INSTDIR"], instdir,
        "the reinstall moved $INSTDIR"
    );
    // Every register but $4 (and $0, the uninstaller's exit code, and $R1, its command) kept,
    // and the stack balanced.
    for register in registers() {
        if register != "$0" && register != "$R1" {
            assert_eq!(machine.vars[&register], format!("kept {register}"));
        }
    }
    assert!(machine.stack.is_empty(), "{:?}", machine.stack);
    // Every macro it reached ran, real or faked.
    assert!(machine.reached.is_empty(), "{:?}", machine.reached);
    machine.said.clear();
    machine.run(&parse_all(
        &["!insertmacro NSIS_HOOK_PREINSTALL".to_owned()],
    ));
    let mismatch_line = "DetailPrint $(previousInstallKeyMismatch)";
    let ignored_line = "DetailPrint $(previousInstallIgnored)";
    let mismatch = machine.said.first().is_some_and(|l| l == mismatch_line);
    let told = machine.said.last().is_some_and(|l| l == ignored_line);
    assert_eq!(
        machine.said.len(),
        usize::from(mismatch) + usize::from(told),
        "{:?}",
        machine.said
    );
    Outcome {
        started,
        instdir,
        told,
        mismatch,
    }
}

fn refused_said(world: &World, told: bool, mismatch: bool) {
    assert_eq!(
        run(world),
        Outcome {
            started: None,
            instdir: world.default().to_owned(),
            told,
            mismatch,
        }
    );
}

fn refused(world: &World) {
    refused_said(world, true, false);
}

/// The command the old uninstaller is started with: per machine the accepted folder's own
/// uninstall.exe, per user `UninstallString`, then the folder.
fn command(world: &World, folder: &str) -> String {
    if world.mode == "perMachine" {
        format!("\"{folder}\\uninstall.exe\" _?={folder}")
    } else {
        format!("{} _?={folder}", world.uninstall_string)
    }
}

fn accepted_said(world: &World, folder: &str, mismatch: bool) {
    assert_eq!(
        run(world),
        Outcome {
            started: Some(command(world, folder)),
            instdir: folder.to_owned(),
            told: false,
            mismatch,
        }
    );
}

fn accepted(world: &World, folder: &str) {
    accepted_said(world, folder, false);
}

#[test]
fn a_planted_folder_elsewhere_is_never_used_per_machine() {
    // Otherwise sound: a fixed drive, a regular uninstall.exe, a closed chain. Per machine the
    // vendor key is ignored and said; with no InstallLocation the default is checked instead,
    // which has no uninstaller here: nothing is started.
    let planted = r"C:\Users\other\x";
    refused_said(&World::new("perMachine", planted), true, true);
    // With a sound default, the default is the one uninstalled and installed into.
    let mut world = World::new("perMachine", planted);
    world.uninstallers.push((exe(MACHINE_DEFAULT), FILE));
    accepted_said(&world, MACHINE_DEFAULT, true);
    // Per user the vendor key is the candidate, and a folder elsewhere is refused.
    refused(&World::new("currentUser", planted));
}

#[test]
fn per_machine_a_vendor_key_elsewhere_gives_way_to_a_sound_install_location() {
    let mut world = World::new("perMachine", r"C:\Users\other\x").located(OTHER);
    world.uninstallers.push((exe(OTHER), FILE));
    accepted_said(&world, OTHER, true);
}

#[test]
fn per_machine_without_install_location_the_sound_default_is_used() {
    accepted(&World::new("perMachine", MACHINE_DEFAULT), MACHINE_DEFAULT);
    // No vendor key either: the default is still the one checked, and nothing is said.
    let mut world = World::new("perMachine", MACHINE_DEFAULT);
    world.previous = None;
    accepted(&world, MACHINE_DEFAULT);
}

#[test]
fn per_machine_the_old_uninstaller_is_the_checked_folders_never_uninstall_string() {
    // UninstallString names another program; the checked folder's own uninstall.exe is started.
    let mut world = World::new("perMachine", MACHINE_DEFAULT);
    world.uninstall_string = r#""C:\Users\other\planted.exe" _?=C:\Users\other"#;
    assert_eq!(
        run(&world).started.as_deref(),
        Some(r#""C:\Program Files\Clauseo\uninstall.exe" _?=C:\Program Files\Clauseo"#)
    );
    // Per user the template's UninstallString stays.
    let mut world = World::new("currentUser", USER_DEFAULT);
    world.uninstall_string = r#""C:\Users\me\AppData\Local\Clauseo\uninstall.exe""#;
    assert_eq!(
        run(&world).started.as_deref(),
        Some(
            r#""C:\Users\me\AppData\Local\Clauseo\uninstall.exe" _?=C:\Users\me\AppData\Local\Clauseo"#
        )
    );
}

#[test]
fn only_x64_or_arm64_builds_of_the_two_built_modes_compile_the_check() {
    let code = hook_code();
    let guard = macro_body(&code, "CLAUSEO_PREVIOUS_DIR_GUARD");
    for part in [
        "!if \"${INSTALLMODE}\" != \"currentUser\"",
        "!if \"${ARCH}\" != \"x64\"",
        "!if \"${ARCH}\" != \"arm64\"",
    ] {
        line_with(guard, &[part]);
    }
    let errors = guard.iter().filter(|l| l.starts_with("!error ")).count();
    assert_eq!(errors, 2, "{guard:?}");
    // Each !error survives the preprocessor only in the build it refuses.
    let kept = |mode: &str, arch: &str| {
        let mut defines = mode_defines(mode);
        defines.insert("ARCH".to_owned(), arch.to_owned());
        preprocess(guard, &defines)
            .iter()
            .filter(|l| l.starts_with("!error "))
            .count()
    };
    assert_eq!(kept("perMachine", "x64"), 0);
    assert_eq!(kept("perMachine", "arm64"), 0);
    assert_eq!(kept("currentUser", "x86"), 0);
    assert_eq!(kept("perMachine", "x86"), 1);
    assert_eq!(kept("both", "x64"), 1);
}

#[test]
fn a_unc_or_device_path_is_refused_even_as_install_location() {
    for path in [
        r"\\server\share\Clauseo",
        r"\\?\C:\Program Files\Clauseo",
        r"\\.\C:\Program Files\Clauseo",
        r"C:\Program Files\Clauseo\..\..\Users\other",
        r"C:\Program Files\.\Clauseo",
        r"C:\Program Files/Clauseo",
        r"Program Files\Clauseo",
        r"1:\Clauseo",
    ] {
        refused(&World::new("perMachine", path).located(path));
    }
}

#[test]
fn a_folder_on_a_drive_that_isnt_fixed_is_refused() {
    let mut world = World::new("perMachine", r"E:\Clauseo").located(r"E:\Clauseo");
    world.not_fixed.push(r"E:\");
    refused(&world);
    // The same folder on a fixed drive is accepted: the drive is what refused it.
    world.not_fixed.clear();
    accepted(&world, r"E:\Clauseo");
}

#[test]
fn a_folder_without_a_regular_uninstaller_is_refused() {
    let mut world = World::new("perMachine", MACHINE_DEFAULT);
    world.uninstallers.clear();
    refused(&world);
    for attributes in [FOLDER, LINK] {
        world.uninstallers = vec![(exe(MACHINE_DEFAULT), attributes)];
        refused(&world);
    }
}

#[test]
fn a_folder_whose_chain_others_can_change_is_refused_per_machine() {
    let mut world = World::new("perMachine", MACHINE_DEFAULT);
    world.open_chains.push(exe(MACHINE_DEFAULT));
    refused(&world);
}

#[test]
fn the_default_folder_with_everything_right_is_accepted_and_uninstalled_in_place() {
    accepted(&World::new("perMachine", MACHINE_DEFAULT), MACHINE_DEFAULT);
    // A vendor key differing only in case and one trailing backslash is no mismatch; the folder
    // used is the default's own spelling.
    let world = World::new("perMachine", r"c:\program files\CLAUSEO\");
    accepted(&world, MACHINE_DEFAULT);
    // An InstallLocation spelled so is used as it is, without the backslash.
    let world = World::new("perMachine", r"c:\program files\CLAUSEO\")
        .located(r"c:\program files\CLAUSEO\");
    accepted(&world, r"c:\program files\CLAUSEO");
}

#[test]
fn install_location_names_another_sound_folder_which_is_accepted() {
    let world = World::new("perMachine", OTHER).located(OTHER);
    accepted(&world, OTHER);
    // With a trailing backslash in the key, and unquoted.
    let mut world = World::new("perMachine", OTHER);
    world.install_location = Some(format!("{OTHER}\\"));
    accepted(&world, OTHER);
    // Without the key the vendor's folder is ignored and said, and the default (no uninstaller
    // here) is refused.
    refused_said(&World::new("perMachine", OTHER), true, true);
}

#[test]
fn per_user_the_default_is_accepted_and_another_folder_refused() {
    // The chain isn't the per-user installer's rule: an open one changes nothing.
    let mut world = World::new("currentUser", USER_DEFAULT);
    world.open_chains.push(exe(USER_DEFAULT));
    accepted(&world, USER_DEFAULT);
    refused(&World::new("currentUser", OTHER));
    refused(&World::new("currentUser", OTHER).located(r"D:\Apps\Elsewhere"));
    accepted(&World::new("currentUser", OTHER).located(OTHER), OTHER);
}

#[test]
fn no_previous_folder_is_a_fresh_install_that_says_nothing() {
    for mode in ["perMachine", "currentUser"] {
        let mut world = World::new(mode, MACHINE_DEFAULT);
        world.previous = None;
        world.uninstallers.clear();
        refused_said(&world, false, false);
    }
}

#[test]
fn each_use_checks_again_so_a_change_between_them_is_still_caught() {
    // The folder passes when $INSTDIR is set from it in .onInit, then its chain opens before the
    // reinstall page starts the old uninstaller: the second use checks again and refuses, and
    // $INSTDIR goes back to the default.
    let world = World::new("perMachine", OTHER).located(OTHER);
    let mut machine = machine(&world);
    restore(&mut machine, world.mode);
    assert_eq!(machine.vars["$INSTDIR"], OTHER);
    assert_eq!(machine.vars["$ClauseoPreviousIgnored"], "0");
    let mut changed = world.clone();
    changed.open_chains.push(exe(OTHER));
    load(&mut machine, &changed);
    reinstall(&mut machine, world.mode);
    assert_eq!(machine.said, ["Goto reinst_done"]);
    assert_eq!(machine.vars["$INSTDIR"], MACHINE_DEFAULT);
    assert_eq!(machine.vars["$ClauseoPreviousIgnored"], "1");
    // Per user, the vendor key swapped after the first use is caught the same way.
    let world = World::new("currentUser", USER_DEFAULT);
    let mut machine = super::previous_dir::machine(&world);
    restore(&mut machine, world.mode);
    assert_eq!(machine.vars["$INSTDIR"], USER_DEFAULT);
    let mut swapped = world.clone();
    swapped.previous = Some(r"C:\Users\other\x");
    swapped.uninstallers.push((exe(r"C:\Users\other\x"), FILE));
    load(&mut machine, &swapped);
    reinstall(&mut machine, world.mode);
    assert_eq!(machine.said, ["Goto reinst_done"]);
    // Per machine, a vendor key swapped after the first use changes nothing: InstallLocation
    // is still the folder used, and the swap is said.
    let world = World::new("perMachine", OTHER).located(OTHER);
    let mut machine = super::previous_dir::machine(&world);
    restore(&mut machine, world.mode);
    let mut swapped = world.clone();
    swapped.previous = Some(r"C:\Users\other\x");
    swapped.uninstallers.push((exe(r"C:\Users\other\x"), FILE));
    load(&mut machine, &swapped);
    machine.said.clear();
    reinstall(&mut machine, world.mode);
    assert_eq!(
        machine.said,
        [format!("ExecWait {}", command(&world, OTHER))]
    );
    assert_eq!(machine.vars["$ClauseoPreviousKeyMismatch"], "1");
}

#[test]
fn the_template_reads_the_previous_folder_only_where_a_guard_follows() {
    let text = read(TEMPLATE);
    let reads: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("\"${MANUPRODUCTKEY}\" \"\"") && l.starts_with("ReadRegStr"))
        .collect();
    assert_eq!(reads, [READ_PREVIOUS, READ_PREVIOUS]);
    // Each block only inserts the guard (and, on the reinstall page, the skip), behind
    // !ifmacrodef so builds without hooks.nsh keep Tauri's behaviour.
    let (blocks, _) = split_blocks(&text);
    let code = |name: &str| -> Vec<String> {
        block_named(&blocks, name)
            .lines
            .iter()
            .filter(|l| !l.is_empty() && !l.starts_with(';'))
            .cloned()
            .collect()
    };
    assert_eq!(
        code("restore-location-guard"),
        [
            "!ifmacrodef CLAUSEO_PREVIOUS_DIR_GUARD",
            "!insertmacro CLAUSEO_PREVIOUS_DIR_GUARD",
            "!endif",
        ]
    );
    assert_eq!(
        code("reinstall-guard"),
        [
            "!ifmacrodef CLAUSEO_PREVIOUS_DIR_GUARD",
            "!insertmacro CLAUSEO_PREVIOUS_DIR_GUARD",
            "${If} $4 == \"\"",
            "StrCpy $0 0",
            "BringToFront",
            "Goto reinst_done",
            "${EndIf}",
            "!insertmacro CLAUSEO_PREVIOUS_UNINSTALLER",
            "!else",
            "ReadRegStr $R1 SHCTX \"${UNINSTKEY}\" \"UninstallString\"",
            "!endif",
        ]
    );
    // The skip lands on the label after the checks of $0, so a skipped uninstall shows no box.
    let lines = function("PageLeaveReinstall");
    let box_at = line_with(
        &lines,
        &["MessageBox MB_ICONEXCLAMATION \"$(unableToUninstall)\""],
    );
    let done = lines.iter().position(|l| l == "reinst_done:").unwrap();
    assert!(box_at < done);
}

#[test]
fn the_native_checks_read_the_path_itself_and_say_no_box() {
    let code = hook_code();
    assert_eq!(
        macro_body(&code, "CLAUSEO_DRIVE_TYPE"),
        ["System::Call 'kernel32::GetDriveTypeW(w r1) i .r2'"]
    );
    assert_eq!(
        macro_body(&code, "CLAUSEO_FILE_ATTRIBUTES"),
        ["System::Call 'kernel32::GetFileAttributesW(w r1) i .r2'"]
    );
    assert_eq!(
        preprocess(
            macro_body(&code, "CLAUSEO_PREVIOUS_UNINSTALLER"),
            &mode_defines("perMachine")
        ),
        ["StrCpy $R1 '\"$4\\uninstall.exe\"'"]
    );
    // The check, the guard and the details line show no box and stop nothing.
    for name in [
        "CLAUSEO_INSTALL_LOCATION",
        "CLAUSEO_PREVIOUS_DIR_OK",
        "CLAUSEO_PREVIOUS_DIR_GUARD",
        "NSIS_HOOK_PREINSTALL",
    ] {
        for line in macro_body(&code, name) {
            for word in ["MessageBox", "Abort", "Quit", "Goto", "Exec"] {
                assert!(!line.contains(word), "{name}: {line}");
            }
        }
    }
    // What the check borrows it gives back, in reverse, $4 (its answer) aside.
    let body = macro_body(&code, "CLAUSEO_PREVIOUS_DIR_OK");
    let pushed: Vec<&str> = body
        .iter()
        .filter_map(|l| l.strip_prefix("Push "))
        .collect();
    let mut popped: Vec<&str> = body.iter().filter_map(|l| l.strip_prefix("Pop ")).collect();
    popped.reverse();
    assert_eq!(pushed, popped);
    assert!(!pushed.contains(&"$4"));
}

#[test]
fn the_details_line_is_in_english_and_hindi() {
    let english = read("windows/nsis/English.nsh");
    let line = "LangString previousInstallIgnored ${LANG_ENGLISH} \"Clauseo found an earlier \
                install in a folder it couldn't confirm, so it installed fresh in its usual \
                folder.\"";
    assert!(english.lines().any(|l| l == line), "{line}");
    let hindi = read("windows/nsis/Hindi.nsh");
    for key in ["previousInstallIgnored", "previousInstallKeyMismatch"] {
        assert!(
            english
                .lines()
                .any(|l| l.starts_with(&format!("LangString {key} ${{LANG_ENGLISH}} \""))),
            "{key}"
        );
        assert!(
            hindi
                .lines()
                .any(|l| l.starts_with(&format!("LangString {key} ${{LANG_HINDI}} \""))),
            "{key}"
        );
    }
}
