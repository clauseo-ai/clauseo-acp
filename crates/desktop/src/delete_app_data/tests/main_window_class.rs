//! Clauseo's main window class is its own, and both installer instances look for it before
//! anything happens (moved out of `template.rs`, which the file-size limit caps).

use super::admin_copy::{admin_code, assert_run, macro_body};
use super::sources::{line_with, read};

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
    // With the Safe workspace off, hooks.nsh defines it instead, for repair mode's wait.
    let off = read("windows/hooks.nsh");
    assert!(
        off.contains(&format!(
            "!define CLAUSEO_MAIN_WINDOW_CLASS \"{}\"",
            crate::MAIN_WINDOW_CLASS
        )),
        "hooks.nsh"
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
            "MessageBox MB_OK|MB_ICONEXCLAMATION|MB_SETFOREGROUND|MB_TOPMOST \"$(adminCopyAppRunning)\"",
            "SetErrorLevel ${CLAUSEO_EXIT_APP_RUNNING}",
        ],
    );
    line_with(&code, &["!define CLAUSEO_EXIT_APP_RUNNING 24"]);
    line_with(&code, &["!define CLAUSEO_ADMIN_APP_RUNNING 97"]);
}
