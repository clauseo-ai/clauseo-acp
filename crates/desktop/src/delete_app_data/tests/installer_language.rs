//! The installer's language (`CLAUSEO_UI_LANGUAGE`, `hooks.nsh`), which the template's block
//! `ui-language` inserts in `.onInit`, before NSIS fixes the pages' language: the lawyer's choice
//! in the app (`HKCU\Software\Clauseo\UiLanguage`, `en` or `hi`) wins over Windows' display
//! language; anything else leaves NSIS's choice. The macro's lines are read from the source and
//! run by `nsis_run` on a fake registry.

use std::collections::HashMap;

use super::admin_copy::macro_body;
use super::nsis_run::{Machine, Record, Value, defines, parse_all, preprocess};
use super::sources::{hook_code, read};
use super::template::{TEMPLATE, block_code, split_blocks};

/// NSIS's language ids, as the template's `MUI_LANGUAGE` lines define them.
const ENGLISH: &str = "1033";
const HINDI: &str = "1081";

/// The defines the macro sees in the installer: `MANUKEY` as the template builds it for Clauseo
/// (the key `ui_language.rs` writes), and the language ids.
fn language_defines() -> HashMap<String, String> {
    let mut defines = defines(&hook_code());
    defines.insert("MANUKEY".to_owned(), "Software\\Clauseo".to_owned());
    defines.insert("LANG_ENGLISH".to_owned(), ENGLISH.to_owned());
    defines.insert("LANG_HINDI".to_owned(), HINDI.to_owned());
    defines
}

/// The macro's lines as the installer compiles them.
fn body(defines: &HashMap<String, String>) -> Vec<String> {
    preprocess(
        &macro_body(&hook_code(), "CLAUSEO_UI_LANGUAGE")[1..],
        defines,
    )
}

/// `CLAUSEO_UI_LANGUAGE` with `$LANGUAGE` at `ENGLISH` (Windows' choice) on the lawyer's key
/// `user`: the `$LANGUAGE` it leaves. It must leave `$R0` and the stack as it found them.
fn language(user: Record) -> String {
    let defines = language_defines();
    let body = body(&defines);
    let mut machine = Machine::new(defines, None);
    machine.user = user;
    machine.set_var("$LANGUAGE", ENGLISH);
    machine.set_var("$R0", "kept");
    machine.run(&parse_all(&body));
    assert_eq!(machine.vars["$R0"], "kept");
    assert!(machine.stack.is_empty(), "{:?}", machine.stack);
    assert!(machine.said.is_empty() && machine.reached.is_empty());
    machine.vars["$LANGUAGE"].clone()
}

/// The record with the app's `UiLanguage` set to `value`.
fn chose(value: Value) -> Vec<(&'static str, Value)> {
    vec![("UiLanguage", value)]
}

#[test]
fn the_installer_language_follows_the_apps_choice() {
    assert_eq!(language(Some(chose(Value::Text("hi")))), HINDI);
    assert_eq!(language(Some(chose(Value::Text("en")))), ENGLISH);
}

#[test]
fn without_the_apps_choice_the_installer_language_stays_windows() {
    // No key (the first install), the key without the value, an empty value.
    assert_eq!(language(None), ENGLISH);
    assert_eq!(language(Some(Vec::new())), ENGLISH);
    assert_eq!(language(Some(chose(Value::Text("")))), ENGLISH);
    // Anything the app doesn't write, or a value that isn't text.
    for other in ["xx", "HI", "hi-IN", " hi"] {
        assert_eq!(
            language(Some(chose(Value::Text(other)))),
            ENGLISH,
            "{other}"
        );
    }
    assert_eq!(language(Some(chose(Value::Unreadable))), ENGLISH);
}

#[test]
fn a_hindi_windows_installer_language_follows_english_chosen_in_the_app() {
    let defines = language_defines();
    let body = body(&defines);
    for (user, wanted) in [(Some(chose(Value::Text("en"))), ENGLISH), (None, HINDI)] {
        let mut machine = Machine::new(defines.clone(), None);
        machine.user = user;
        machine.set_var("$LANGUAGE", HINDI);
        machine.run(&parse_all(&body));
        assert_eq!(machine.vars["$LANGUAGE"], wanted);
    }
}

#[test]
fn the_installer_language_macro_reads_only_the_apps_value() {
    let code = hook_code();
    assert_eq!(
        macro_body(&code, "CLAUSEO_UI_LANGUAGE")[1..],
        [
            "!ifndef LANG_HINDI",
            "!error \"hooks.nsh needs Hindi in bundle.windows.nsis.languages\"",
            "!endif",
            "Push $R0",
            "ReadRegStr $R0 HKCU \"${MANUKEY}\" \"UiLanguage\"",
            "${If} $R0 S== \"hi\"",
            "StrCpy $LANGUAGE ${LANG_HINDI}",
            "${ElseIf} $R0 S== \"en\"",
            "StrCpy $LANGUAGE ${LANG_ENGLISH}",
            "${EndIf}",
            "Pop $R0",
        ]
    );
    // MANUKEY is the template's "Software\${MANUFACTURER}", the key the app writes.
    let template = read(TEMPLATE);
    assert!(template.contains("!define MANUKEY \"Software\\${MANUFACTURER}\""));
}

#[test]
fn the_ui_language_block_sets_the_installer_language_before_the_selector() {
    let text = read(TEMPLATE);
    let (blocks, _) = split_blocks(&text);
    assert_eq!(
        block_code(&blocks, "ui-language"),
        [
            "!ifmacrodef CLAUSEO_UI_LANGUAGE",
            "!insertmacro CLAUSEO_UI_LANGUAGE",
            "!endif",
        ]
    );
    // In .onInit, after the admin section and before the language selector and FunctionEnd.
    let lines: Vec<&str> = text.split('\n').map(str::trim).collect();
    let at = |wanted: &str, from: usize| {
        from + lines[from..]
            .iter()
            .position(|line| *line == wanted)
            .unwrap_or_else(|| panic!("no {wanted:?}"))
    };
    let start = at("Function .onInit", 0);
    let admin = at("; <<< clauseo: admin-section", start);
    let block = at("; >>> clauseo: ui-language", start);
    let selector = at("!if \"${DISPLAYLANGUAGESELECTOR}\" == \"true\"", start);
    let end = at("FunctionEnd", start);
    assert!(admin < block && block < selector && selector < end);
}
