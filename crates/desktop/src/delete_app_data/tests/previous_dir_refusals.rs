//! The refusals of the previous install folder, moved out of `previous_dir.rs` to keep both files
//! under the line limit: a UNC or device path, a folder on a drive that isn't fixed, a folder
//! without a regular `uninstall.exe`, a chain others can change, a change between the two uses,
//! and the native checks that read the path itself. The harness and helpers are `previous_dir`'s.

use super::nsis_run::preprocess;
use super::previous_dir::{
    FILE, FOLDER, LINK, MACHINE_DEFAULT, OTHER, USER_DEFAULT, World, accepted, command, exe, load,
    machine, macro_body, mode_defines, refused, reinstall, restore,
};
use super::sources::hook_code;

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
