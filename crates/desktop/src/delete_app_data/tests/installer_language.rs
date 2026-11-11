//! The installer's language and the app's `UiLanguage`, in `hooks.nsh`.
//! `CLAUSEO_UI_LANGUAGE`, which the template's block `ui-language` inserts in `.onInit`, before
//! NSIS fixes the pages' language,
//! takes the lawyer's choice in the app (`HKCU\Software\Clauseo\UiLanguage`, `en` or `hi`); with no
//! choice, and not silent, it asks in English or Hindi, and `Abort`s on cancel. Silent, it keeps
//! NSIS's own choice. `NSIS_HOOK_POSTINSTALL` writes `UiLanguage` once, when the app has none, with
//! the language the installer ran in, in both installers. The macros' lines are read from the
//! source and run by `nsis_run` on a fake registry and a fake `LangDLL`.

use std::collections::HashMap;

use super::admin_copy::macro_body;
use super::nsis_run::{Machine, Record, Value, defines, parse_all, preprocess};
use super::sources::{hook_code, line_with, read};
use super::template::{TEMPLATE, block_code, split_blocks};

/// NSIS's language ids, as the template's `MUI_LANGUAGE` lines define them.
const ENGLISH: &str = "1033";
const HINDI: &str = "1081";

/// The dialog's shown text, as the machine records it when it runs.
const ASKED: &str = "LangDialog Installer Language";

/// The defines the macros see in the installer: `MANUKEY` as the template builds it for Clauseo
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

/// `CLAUSEO_UI_LANGUAGE` with `$LANGUAGE` at `installer` (NSIS's own choice) on the lawyer's key
/// `user`, run silent or not, the dialog answering `answer`. It must leave `$R0` and the stack as
/// it found them, unless `Abort` ends it.
fn choose(user: Record, silent: bool, answer: &str, installer: &str) -> Machine {
    let defines = language_defines();
    let body = body(&defines);
    let mut machine = Machine::new(defines, None);
    machine.user = user;
    machine.silent = silent;
    machine.answer = answer.to_owned();
    machine.set_var("$LANGUAGE", installer);
    machine.set_var("$R0", "kept");
    machine.run(&parse_all(&body));
    if !machine.aborted {
        assert_eq!(machine.vars["$R0"], "kept");
        assert!(machine.stack.is_empty(), "{:?}", machine.stack);
    }
    assert!(machine.written.is_empty() && machine.reached.is_empty());
    machine
}

/// The record with the app's `UiLanguage` set to `value`.
fn chose(value: Value) -> Vec<(&'static str, Value)> {
    vec![("UiLanguage", value)]
}

/// No choice in the app: no key, the key without the value, an empty value, and a value that
/// isn't text (its read fails, which leaves "" as well).
fn absent() -> Vec<Record> {
    vec![
        None,
        Some(Vec::new()),
        Some(chose(Value::Text(""))),
        Some(chose(Value::Unreadable)),
    ]
}

/// A value the app doesn't write: neither `hi` nor `en`, nor the empty text.
fn unknown() -> Vec<Record> {
    ["fr", "xx", "HI", "hi-IN", " hi"]
        .into_iter()
        .map(|other| Some(chose(Value::Text(other))))
        .collect()
}

/// The pairs the dialog read, as (name, id).
fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(name, id)| ((*name).to_owned(), (*id).to_owned()))
        .collect()
}

#[test]
fn the_apps_choice_is_used_and_nothing_is_asked() {
    for silent in [false, true] {
        let hindi = choose(Some(chose(Value::Text("hi"))), silent, "", ENGLISH);
        assert_eq!(hindi.vars["$LANGUAGE"], HINDI);
        let english = choose(Some(chose(Value::Text("en"))), silent, "", HINDI);
        assert_eq!(english.vars["$LANGUAGE"], ENGLISH);
        for machine in [hindi, english] {
            assert!(machine.said.is_empty() && machine.asked.is_empty());
            assert!(!machine.aborted);
        }
    }
}

#[test]
fn a_first_install_asks_english_or_hindi_and_takes_the_answer() {
    let mut users = absent();
    users.extend(unknown());
    for user in users {
        for installer in [ENGLISH, HINDI] {
            // Hindi first, then English: the two languages, each as its id.
            let hindi = choose(user.clone(), false, HINDI, installer);
            assert_eq!(hindi.vars["$LANGUAGE"], HINDI);
            assert_eq!(hindi.said, [ASKED]);
            assert_eq!(
                hindi.asked,
                pairs(&[("हिन्दी", HINDI), ("English", ENGLISH)])
            );
            assert!(!hindi.aborted);
            let english = choose(user.clone(), false, ENGLISH, installer);
            assert_eq!(english.vars["$LANGUAGE"], ENGLISH);
        }
    }
}

#[test]
fn a_silent_install_asks_nothing_and_keeps_nsiss_own_choice() {
    let mut users = absent();
    users.extend(unknown());
    for user in users {
        for installer in [ENGLISH, HINDI] {
            let machine = choose(user.clone(), true, HINDI, installer);
            assert_eq!(machine.vars["$LANGUAGE"], installer);
            assert!(machine.said.is_empty() && machine.asked.is_empty());
            assert!(!machine.aborted);
        }
    }
}

#[test]
fn cancelling_the_dialog_aborts_the_installer() {
    let machine = choose(None, false, "cancel", ENGLISH);
    assert!(machine.aborted);
    assert_eq!(machine.said, [ASKED, "Abort"]);
}

#[test]
fn the_installer_language_macro_chooses_and_asks_only_without_the_apps_choice() {
    assert_eq!(
        macro_body(&hook_code(), "CLAUSEO_UI_LANGUAGE")[1..],
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
            "${ElseIfNot} ${Silent}",
            "Push \"\"",
            "Push ${LANG_ENGLISH}",
            "Push \"English\"",
            "Push ${LANG_HINDI}",
            "Push \"हिन्दी\"",
            "Push \"A\"",
            "LangDLL::LangDialog \"Installer Language\" \"Please select a language.\"",
            "Pop $LANGUAGE",
            "${If} $LANGUAGE S== \"cancel\"",
            "Abort",
            "${EndIf}",
            "${EndIf}",
            "Pop $R0",
        ]
    );
    // MANUKEY is the template's "Software\${MANUFACTURER}", the key the app writes.
    let template = read(TEMPLATE);
    assert!(template.contains("!define MANUKEY \"Software\\${MANUFACTURER}\""));
}

/// `NSIS_HOOK_POSTINSTALL`'s language lines, from its first `Push $R0` to the `Pop $R0` that
/// balances it, preprocessed for the `mode` build, with the defines they need.
fn postinstall_language(mode: &str) -> (HashMap<String, String>, Vec<String>) {
    let code = hook_code();
    let post = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    let start = line_with(post, &["Push $R0"]);
    let end = start + line_with(&post[start..], &["Pop $R0"]) + 1;
    let mut defines = language_defines();
    defines.insert("INSTALLMODE".to_owned(), mode.to_owned());
    // MANUPRODUCTKEY is ${MANUKEY}\${PRODUCTNAME}, the product name being Clauseo.
    defines.insert(
        "MANUPRODUCTKEY".to_owned(),
        "Software\\Clauseo\\Clauseo".to_owned(),
    );
    let body = preprocess(&post[start..end], &defines);
    (defines, body)
}

/// The registry writes the language lines make in the `mode` build, with `$LANGUAGE` at
/// `installer` (a language id) on the lawyer's key `user`.
fn writes(mode: &str, installer: &str, user: Record) -> Vec<String> {
    let (defines, body) = postinstall_language(mode);
    let mut machine = Machine::new(defines, None);
    machine.user = user;
    machine.set_var("$LANGUAGE", installer);
    machine.set_var("$R0", "kept");
    machine.set_var("$R1", "kept");
    machine.run(&parse_all(&body));
    // The registers are put back, and nothing is said or reached.
    assert_eq!(machine.vars["$R0"], "kept");
    assert_eq!(machine.vars["$R1"], "kept");
    assert!(machine.stack.is_empty(), "{:?}", machine.stack);
    assert!(machine.said.is_empty() && machine.reached.is_empty());
    machine.written
}

/// The write of the app's `UiLanguage` to `value`.
fn ui_language(value: &str) -> String {
    format!("HKCU: Software\\Clauseo, UiLanguage = {value}")
}

/// The write of the installer's own language, as the uninstaller reads it (a language id).
fn installer_language(id: &str) -> String {
    format!("HKCU: Software\\Clauseo\\Clauseo, Installer Language = {id}")
}

const MODES: [&str; 2] = ["currentUser", "perMachine"];

#[test]
fn the_installer_records_the_language_it_ran_in_when_the_app_has_none_in_both_modes() {
    for mode in MODES {
        for user in absent() {
            assert_eq!(
                writes(mode, HINDI, user.clone()),
                [ui_language("hi"), installer_language(HINDI)],
                "{mode}"
            );
            assert_eq!(
                writes(mode, ENGLISH, user),
                [ui_language("en"), installer_language(ENGLISH)],
                "{mode}"
            );
        }
    }
}

#[test]
fn the_answer_to_the_dialog_is_what_the_app_is_told_on_a_first_install() {
    for mode in MODES {
        for answer in [HINDI, ENGLISH] {
            let chosen = choose(None, false, answer, ENGLISH).vars["$LANGUAGE"].clone();
            let told = if answer == HINDI { "hi" } else { "en" };
            assert_eq!(writes(mode, &chosen, None)[0], ui_language(told), "{mode}");
        }
    }
}

#[test]
fn an_apps_choice_is_never_written_over_in_both_modes() {
    for mode in MODES {
        let user = Some(chose(Value::Text("hi")));
        assert_eq!(
            writes(mode, ENGLISH, user),
            [installer_language(HINDI)],
            "{mode}"
        );
        let user = Some(chose(Value::Text("en")));
        assert_eq!(
            writes(mode, HINDI, user),
            [installer_language(ENGLISH)],
            "{mode}"
        );
    }
}

#[test]
fn an_unknown_value_is_left_as_it_is_in_both_modes() {
    for mode in MODES {
        for user in unknown() {
            for installer in [HINDI, ENGLISH] {
                assert_eq!(
                    writes(mode, installer, user.clone()),
                    [installer_language(installer)],
                    "{mode} {installer}"
                );
            }
        }
    }
}

#[test]
fn the_absent_branch_compares_text_not_numbers() {
    // `== ""` compares numbers, where "fr" reads as 0 and would match the empty branch too.
    let (_, body) = postinstall_language("currentUser");
    assert!(
        body.contains(&"${ElseIf} $R0 S== \"\"".to_owned()),
        "{body:?}"
    );
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
