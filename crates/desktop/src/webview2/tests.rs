use super::DOWNLOAD_URL;
use super::dialog::{
    APP_ICON, CLOSE, DOWNLOAD, MESSAGE_BOX_OK, Next, after_message_box, buttons, main_icon,
    message_box_labels, message_box_text, next,
};
use super::version::{MINIMUM, Verdict, Version, verdict};
use super::words::{MISSING_EN, MISSING_HI, TOO_OLD_EN, TOO_OLD_HI, Words, fits, words};
use clauseo_protocol::Language;
use serde_json::Value;
use std::path::Path;

mod installer_languages;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_dir().join(relative)).unwrap()
}

fn tauri_conf() -> Value {
    serde_json::from_str(&read("tauri.conf.json")).unwrap()
}

fn minimum() -> Version {
    Version::parse(MINIMUM).unwrap()
}

#[test]
fn the_minimum_is_tauri_confs_minimum_webview2_version() {
    let conf = tauri_conf();
    let configured = conf["bundle"]["windows"]["minimumWebview2Version"]
        .as_str()
        .unwrap();
    assert_eq!(MINIMUM, configured);
}

#[test]
fn no_runtime_is_missing() {
    assert_eq!(verdict(None, minimum()), Verdict::Missing);
    assert_eq!(verdict(Some(""), minimum()), Verdict::Missing);
    assert_eq!(verdict(Some("  "), minimum()), Verdict::Missing);
}

#[test]
fn an_older_runtime_is_too_old() {
    for older in [
        "111.0.1661.40",
        "110.0.1587.69",
        "60",
        "111",
        "110.0.1587.69 beta",
    ] {
        assert_eq!(verdict(Some(older), minimum()), Verdict::TooOld, "{older}");
    }
}

#[test]
fn the_minimum_itself_starts() {
    assert_eq!(verdict(Some("111.0.1661.41"), minimum()), Verdict::Start);
    assert_eq!(verdict(Some("120"), minimum()), Verdict::Start);
    // Clean Windows 11's runtime (the ARM64 VM's win11-clean, 2026-11-09).
    assert_eq!(verdict(Some("152.0.4191.66"), minimum()), Verdict::Start);
}

#[test]
fn a_newer_runtime_starts() {
    for newer in [
        "155.0.3485.54",
        "156.0.0.0",
        "200.1.2.3 canary",
        "1000.0.0.0",
    ] {
        assert_eq!(verdict(Some(newer), minimum()), Verdict::Start, "{newer}");
    }
}

#[test]
fn a_version_it_cannot_read_lets_the_app_start() {
    for malformed in [
        "abc",
        "155.0.x.0",
        "155..0.0",
        "155.0.0.0.0",
        "-1.0.0.0",
        "+156.0.0.0",
        "99999999999.0.0.0",
        "v156",
    ] {
        assert_eq!(Version::parse(malformed), None, "{malformed}");
        assert_eq!(
            verdict(Some(malformed), minimum()),
            Verdict::Start,
            "{malformed}"
        );
    }
}

#[test]
fn versions_compare_part_by_part() {
    let parse = |text| Version::parse(text).unwrap();
    assert!(parse("155.0.0.10") > parse("155.0.0.9"));
    assert!(parse("155.1.0.0") > parse("155.0.9999.9999"));
    assert_eq!(parse("155.0"), parse("155.0.0.0"));
}

/// Every set, for the tests that hold every set to the same thing.
fn sets() -> [&'static Words; 4] {
    [&MISSING_EN, &TOO_OLD_EN, &MISSING_HI, &TOO_OLD_HI]
}

/// Each verdict's set in English, then in Hindi.
fn pairs() -> [(&'static Words, &'static Words); 2] {
    [(&MISSING_EN, &MISSING_HI), (&TOO_OLD_EN, &TOO_OLD_HI)]
}

#[test]
fn each_verdict_picks_its_set() {
    let instruction = |verdict, language| words(verdict, language).map(|set| set.instruction);
    assert_eq!(
        instruction(Verdict::Missing, Language::En),
        Some(MISSING_EN.instruction)
    );
    assert_eq!(
        instruction(Verdict::TooOld, Language::En),
        Some(TOO_OLD_EN.instruction)
    );
    assert!(words(Verdict::Start, Language::En).is_none());
    assert!(words(Verdict::Start, Language::Hi).is_none());
    // Through the check: a missing runtime and an old one each get their own set.
    let missing = words(verdict(None, minimum()), Language::En);
    let too_old = words(verdict(Some("110.0.1587.69"), minimum()), Language::En);
    assert_eq!(
        missing.map(|set| set.instruction),
        Some(MISSING_EN.instruction)
    );
    assert_eq!(
        too_old.map(|set| set.instruction),
        Some(TOO_OLD_EN.instruction)
    );
    assert_eq!(
        MISSING_EN.instruction,
        "Clauseo needs Microsoft Edge WebView2 to open"
    );
    assert_eq!(
        TOO_OLD_EN.instruction,
        "Clauseo needs a newer Microsoft Edge WebView2"
    );
}

/// The registry's language picks the words: Hindi for Hindi, English for English, per verdict.
#[test]
fn the_language_picks_the_words() {
    let missing_hi = words(Verdict::Missing, Language::Hi).unwrap();
    assert_eq!(missing_hi.action, Some("WebView2 डाउनलोड करें"));
    assert_eq!(missing_hi.close, "बंद करें");
    let too_old_hi = words(Verdict::TooOld, Language::Hi).unwrap();
    assert_eq!(too_old_hi.instruction, TOO_OLD_HI.instruction);
    assert_eq!(too_old_hi.action, None);
    let missing_en = words(Verdict::Missing, Language::En).unwrap();
    assert_eq!(missing_en.action, Some("Download WebView2"));
    assert_eq!(
        message_box_text(missing_hi),
        format!("{}\n\n{}", MISSING_HI.instruction, MISSING_HI.content)
    );
}

#[test]
fn every_string_is_there() {
    for set in sets() {
        for text in set.all() {
            assert_ne!(text.trim(), "");
            assert_eq!(text.trim(), text);
        }
        assert_eq!(set.title, "Clauseo");
    }
    // Missing has its action; TooOld has none, so one string fewer, and no empty one.
    for (en, hi) in pairs() {
        assert_eq!(en.all().len(), hi.all().len(), "{}", en.instruction);
    }
    assert_eq!(MISSING_EN.all().len(), 6);
    assert_eq!(TOO_OLD_EN.all().len(), 5);
}

/// Each Hindi line is Devanagari (the title is the app's name in both), with a danda where the
/// English has a full stop; the English keeps the app's voice.
#[test]
fn the_hindi_is_hindi_and_the_english_keeps_the_voice() {
    for (en_set, hi_set) in pairs() {
        for (en, hi) in en_set.all().into_iter().zip(hi_set.all()).skip(1) {
            assert!(
                hi.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c)),
                "{hi}"
            );
            assert_eq!(en.ends_with('.'), hi.ends_with('।'), "{hi}");
        }
        for en in en_set.all() {
            for word in crate::dialogs::words::OFF_VOICE {
                assert!(!en.to_lowercase().contains(word), "{en}");
            }
        }
    }
}

/// Every word of the dialog, in both languages, appears verbatim in its review home,
/// `ui/app/src/review/registry-nd.tsx`: Missing's under ND-16 to ND-21, then `TooOld`'s own
/// three lines under ND-22 to ND-24. `TooOld`'s title and Close are Missing's (ND-16, ND-21).
#[test]
fn the_dialogs_words_are_the_reviewed_copy() {
    let registry =
        std::fs::read_to_string(manifest_dir().join("../../ui/app/src/review/registry-nd.tsx"))
            .unwrap();
    let ids = (16..).zip(MISSING_EN.all().into_iter().zip(MISSING_HI.all()));
    let lines = |set: &'static Words| [set.instruction, set.content, set.footer];
    let too_old = (22..).zip(lines(&TOO_OLD_EN).into_iter().zip(lines(&TOO_OLD_HI)));
    assert_eq!(
        (TOO_OLD_EN.title, TOO_OLD_EN.close),
        (MISSING_EN.title, MISSING_EN.close)
    );
    assert_eq!(
        (TOO_OLD_HI.title, TOO_OLD_HI.close),
        (MISSING_HI.title, MISSING_HI.close)
    );
    for (n, (en, hi)) in ids.chain(too_old) {
        let id = format!("\"ND-{n}\"");
        assert!(registry.contains(&id), "no {id}");
        for text in [en, hi] {
            assert!(
                registry.contains(&format!("\"{text}\"")),
                "{id} lacks {text}"
            );
        }
    }
}

#[test]
fn only_missing_offers_the_download() {
    // Missing's button takes the id the link opens (DOWNLOAD_URL); TooOld shows only Close.
    assert_eq!(
        buttons(&MISSING_EN),
        [(DOWNLOAD, "Download WebView2"), (CLOSE, "Close")]
    );
    assert_eq!(
        buttons(&MISSING_HI),
        [(DOWNLOAD, "WebView2 डाउनलोड करें"), (CLOSE, "बंद करें")]
    );
    assert_eq!(buttons(&TOO_OLD_EN), [(CLOSE, "Close")]);
    assert_eq!(buttons(&TOO_OLD_HI), [(CLOSE, "बंद करें")]);
    assert_eq!(next::<()>(&Ok(DOWNLOAD)), Next::Download);
}

#[test]
fn a_set_without_an_action_shows_only_close() {
    let only_close = Words {
        action: None,
        ..TOO_OLD_EN
    };
    assert_eq!(buttons(&only_close), [(CLOSE, "Close")]);
    assert!(buttons(&only_close).iter().all(|(id, _)| *id != DOWNLOAD));
    assert_eq!(next::<()>(&Ok(CLOSE)), Next::Close);
}

#[test]
fn the_download_link_is_microsofts_bootstrapper_page() {
    assert_eq!(
        DOWNLOAD_URL,
        "https://go.microsoft.com/fwlink/p/?LinkId=2124703"
    );
}

#[test]
fn the_download_button_downloads_and_every_other_answer_closes() {
    assert_eq!(next::<()>(&Ok(DOWNLOAD)), Next::Download);
    assert_eq!(next::<()>(&Ok(CLOSE)), Next::Close);
    // IDCANCEL: Esc or the title bar's close button.
    assert_eq!(next::<()>(&Ok(2)), Next::Close);
    assert_eq!(next::<()>(&Ok(0)), Next::Close);
}

#[test]
fn a_failed_task_dialog_falls_back_to_the_message_box() {
    assert_eq!(next(&Err("E_INVALIDARG")), Next::MessageBox);
}

#[test]
fn the_icon_is_named_only_when_the_exe_holds_it() {
    assert_eq!(main_icon(true), Some(APP_ICON));
    assert_eq!(main_icon(false), None);
    // tauri-build's id, IDI_APPLICATION's value.
    assert_eq!(APP_ICON, 32512);
}

#[test]
fn the_message_box_offers_the_download_with_the_task_dialogs_labels() {
    // #858: the fallback offers the download too, in the words the task dialog uses.
    assert_eq!(
        message_box_labels(&MISSING_EN),
        Some(("Download WebView2", "Close"))
    );
    assert_eq!(
        message_box_labels(&MISSING_HI),
        Some(("WebView2 डाउनलोड करें", "बंद करें"))
    );
    assert_eq!(message_box_labels(&TOO_OLD_EN), None);
    assert_eq!(message_box_labels(&TOO_OLD_HI), None);
    // IDOK's value.
    assert_eq!(MESSAGE_BOX_OK, 1);
    assert_eq!(
        after_message_box(&MISSING_EN, MESSAGE_BOX_OK),
        Next::Download
    );
    // IDCANCEL (Close, Esc, the title bar), or 0 for a box that failed.
    assert_eq!(after_message_box(&MISSING_EN, 2), Next::Close);
    assert_eq!(after_message_box(&MISSING_EN, 0), Next::Close);
    // Too old: OK only closes.
    assert_eq!(after_message_box(&TOO_OLD_EN, MESSAGE_BOX_OK), Next::Close);
}

#[test]
fn the_message_box_says_the_main_line_then_the_body() {
    let missing = words(Verdict::Missing, Language::En).unwrap();
    assert_eq!(
        message_box_text(missing),
        "Clauseo needs Microsoft Edge WebView2 to open\n\n\
         It's free from Microsoft and installs without an administrator."
    );
    assert_eq!(
        message_box_text(words(Verdict::TooOld, Language::En).unwrap()),
        "Clauseo needs a newer Microsoft Edge WebView2\n\n\
         Windows updates it by itself."
    );
}

#[test]
fn the_dialog_is_wide_enough_for_the_main_line_and_no_wider_than_a_small_screen() {
    for set in sets() {
        assert!(fits(set.instruction, set.width), "{}", set.instruction);
        // 150 % on a 1366 px wide screen: about 1.75 px a dialog unit at 100 %.
        assert!(f64::from(set.width) * 1.75 * 1.5 < 1366.0);
    }
    assert!(!fits(MISSING_EN.instruction, 200));
    // One width per language.
    for (en, hi) in pairs() {
        assert_eq!(en.width, MISSING_EN.width);
        assert_eq!(hi.width, MISSING_HI.width);
    }
}

/// `hooks.nsh`'s post-install macro, its code lines trimmed (comments and blank lines left out).
fn postinstall_code() -> Vec<String> {
    let code: Vec<String> = read("windows/hooks.nsh")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .map(str::to_owned)
        .collect();
    let start = code
        .iter()
        .position(|line| line == "!macro NSIS_HOOK_POSTINSTALL")
        .unwrap();
    let end = start
        + code[start..]
            .iter()
            .position(|line| line == "!macroend")
            .unwrap();
    code[start..end].to_vec()
}

#[test]
fn every_install_tells_the_uninstaller_the_apps_language_else_its_own() {
    // MUI_UNGETLANGUAGE in the template's un.onInit reads this value; empty, it shows a picker.
    let body = postinstall_code();
    let at = |wanted: &str| {
        body.iter()
            .position(|line| line == wanted)
            .unwrap_or_else(|| panic!("no {wanted:?} in {body:?}"))
    };
    let own = at("StrCpy $R1 $LANGUAGE");
    let read = at(r#"ReadRegStr $R0 HKCU "${MANUKEY}" "UiLanguage""#);
    let hindi = at(r#"${If} $R0 S== "hi""#);
    let english = at(r#"${ElseIf} $R0 S== "en""#);
    let absent = at("${ElseIf} $R0 S== \"\"");
    let write = at(r#"WriteRegStr HKCU "${MANUPRODUCTKEY}" "Installer Language" $R1"#);
    let per_machine = at(r#"!if "${INSTALLMODE}" == "perMachine""#);
    assert!(own < read && read < hindi && hindi < english && english < absent);
    assert!(absent < write && write < per_machine);
    assert_eq!(body[hindi + 1], "StrCpy $R1 ${LANG_HINDI}");
    assert_eq!(body[english + 1], "StrCpy $R1 ${LANG_ENGLISH}");
    // Both installers: the app's UiLanguage once, when absent, the language the installer ran in.
    assert_eq!(
        body[absent + 1..absent + 4],
        [
            "${If} $LANGUAGE = ${LANG_HINDI}".to_owned(),
            "WriteRegStr HKCU \"${MANUKEY}\" \"UiLanguage\" \"hi\"".to_owned(),
            "${Else}".to_owned(),
        ]
    );
    // HKCU only, nothing written anywhere else: UiLanguage twice (hi, en), the language once.
    let writes: Vec<&String> = body
        .iter()
        .filter(|line| line.starts_with("WriteReg"))
        .collect();
    assert_eq!(writes.len(), 3, "{body:?}");
    assert!(
        writes
            .iter()
            .all(|line| line.starts_with("WriteRegStr HKCU "))
    );
    // The registers are put back.
    assert_eq!(
        body[own - 2..own],
        ["Push $R0".to_owned(), "Push $R1".to_owned()]
    );
    assert_eq!(
        body[write + 1..write + 3],
        ["Pop $R1".to_owned(), "Pop $R0".to_owned()]
    );
}
