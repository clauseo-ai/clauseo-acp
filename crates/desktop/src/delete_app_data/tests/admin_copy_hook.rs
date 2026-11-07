//! The uninstall hook starts the set-up program's admin copy in Program Files only, never the
//! copy beside the app, and never falls back to it (S15 tier B, "Admin copy" in 0049).

use super::sources::{hook_code, line_with, preuninstall};

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
    let info = line_with(body, &["System::Call '*(&l4, i 0x440"]);
    assert!(
        body[info].contains(concat!(
            "w \"${CLAUSEO_REMOVE_VERB}\", w \"${CLAUSEO_ADMIN_EXE}\", ",
            "w \"${CLAUSEO_REMOVE}\", p 0,"
        )),
        "{}",
        body[info]
    );
}
