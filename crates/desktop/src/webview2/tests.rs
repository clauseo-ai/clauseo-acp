use super::DOWNLOAD_URL;
use super::dialog::{APP_ICON, CLOSE, DOWNLOAD, Next, buttons, main_icon, message_box_text, next};
use super::version::{MINIMUM, Verdict, Version, verdict};
use super::words::{MISSING_EN, MISSING_HI, TOO_OLD_EN, TOO_OLD_HI, Words, fits, words};
use clauseo_protocol::Language;
use serde_json::Value;
use std::path::Path;

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

/// The `LangString`s of a language file, in order: each key, its language constant and its text
/// (inside the quotes).
fn lang_strings(relative: &str) -> Vec<(String, String, String)> {
    read(relative)
        .lines()
        .filter_map(|line| line.strip_prefix("LangString "))
        .map(|rest| {
            let mut parts = rest.splitn(3, ' ');
            let key = parts.next().unwrap().to_owned();
            let constant = parts.next().unwrap().to_owned();
            let text = parts.next().unwrap();
            assert!(
                text.len() >= 2 && text.starts_with('"') && text.ends_with('"'),
                "{key}: {text}"
            );
            (key, constant, text[1..text.len() - 1].to_owned())
        })
        .collect()
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

/// The installer's language files, as `customLanguageFiles` names them, each with its NSIS
/// language constant: English first (NSIS's fallback), then Hindi, which is ours (Tauri 2.12.2
/// ships no Hindi).
fn language_files() -> [(String, &'static str); 2] {
    let conf = tauri_conf();
    let nsis = &conf["bundle"]["windows"]["nsis"];
    assert_eq!(nsis["languages"], serde_json::json!(["English", "Hindi"]));
    // No picker: NSIS chooses (windows/hooks.nsh says how).
    assert_ne!(nsis["displayLanguageSelector"], serde_json::json!(true));
    let files = nsis["customLanguageFiles"].as_object().unwrap();
    let mut names: Vec<&String> = files.keys().collect();
    names.sort_unstable();
    assert_eq!(names, ["English", "Hindi"]);
    [
        (
            files["English"].as_str().unwrap().to_owned(),
            "${LANG_ENGLISH}",
        ),
        (files["Hindi"].as_str().unwrap().to_owned(), "${LANG_HINDI}"),
    ]
}

#[test]
fn the_installer_language_files_sit_beside_each_other() {
    let [(english, _), (hindi, _)] = language_files();
    assert_eq!(english, "windows/nsis/English.nsh");
    assert_eq!(hindi, "windows/nsis/Hindi.nsh");
}

#[test]
fn the_installer_language_files_name_the_product_through_nsis() {
    for (file, constant) in language_files() {
        let text = read(&file);
        // Language files are included, not rendered, so a template mark would show as it is.
        for line in text.lines().filter(|line| !line.starts_with(';')) {
            assert!(!line.contains("{{"), "{file}: {line}");
        }
        for key in ["appRunning", "appRunningOkKill", "failedToKillApp"] {
            let prefix = format!("LangString {key} {constant} ");
            let line = text
                .lines()
                .find_map(|line| line.strip_prefix(prefix.as_str()))
                .unwrap_or_else(|| panic!("{file}: no {key}"));
            assert!(line.contains("${PRODUCTNAME}"), "{file} {key}: {line}");
        }
    }
}

#[test]
fn the_installer_language_files_hold_tauris_27_messages_and_clauseos_28_once_each() {
    // Tauri's 27, the eight lines windows/hooks.nsh shows after the app deletes its data or about
    // S15's removal (deleteAppDataFailed, deleteAppDataKept, deleteAppDataKeptFailed,
    // deleteAppDataFailedUnnamed, removeStillRunning, removeSkippedSilent, removeExeMissing and
    // removeAdminCopyRefused), which tests there hold to the hook, its two about the firm-policy
    // folder (policyFolderExists and policyFolderFailed), the installers' setupExeRemoved, its
    // twelve about the admin copy (adminCopyAsk, adminCopyWorking, adminCopyDone,
    // adminCopyDeclined, adminCopyBlocked, adminCopyFailed, adminCopyOtherAccount,
    // adminCopyMachineFailed, adminCopyNewerKept, adminCopyAppRunning, adminCopyStillRunning and
    // adminCopySkippedSilent),
    // the per-machine installer's and uninstaller's about their temporary folders
    // (installTempFailed and uninstallTempFailed), the uninstaller's when it isn't started in
    // place (uninstallNotInPlace), the per-machine installer's details line on an ARM64
    // computer (nativeArm64Notice), and the details line about an old WebView2 (webview2TooOld).
    let [english, hindi] = language_files();
    let mut orders = Vec::new();
    for (file, constant) in [english, hindi] {
        let strings = lang_strings(&file);
        assert_eq!(strings.len(), 55, "{file}");
        let order: Vec<String> = strings.iter().map(|(key, _, _)| key.clone()).collect();
        let mut keys = order.clone();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 55, "{file}");
        assert!(
            strings.iter().all(|(_, used, _)| used == constant),
            "{file}: every string is {constant}'s"
        );
        orders.push(order);
    }
    // The same keys in the same order, so a Tauri upgrade's new key goes into both.
    assert_eq!(orders[0], orders[1]);
}

#[test]
fn the_installer_hindi_file_is_in_hindi() {
    // Until the app thread's Hindi lands, Hindi.nsh holds English.nsh's words: this fails, so the
    // branch can't ship English in the Hindi file. Every value holds a Devanagari character.
    let [_, (hindi, _)] = language_files();
    let english: Vec<String> = lang_strings(&hindi)
        .into_iter()
        .filter(|(_, _, text)| !text.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c)))
        .map(|(key, _, _)| key)
        .collect();
    assert!(english.is_empty(), "{hindi}: no Devanagari in {english:?}");
}

#[test]
fn the_delete_app_data_box_says_clauseos_words_on_one_line() {
    for (file, constant) in language_files() {
        let text = read(&file);
        let prefix = format!("LangString deleteAppData {constant} ");
        let line = text
            .lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .unwrap_or_else(|| panic!("{file}: no deleteAppData"));
        let words = line.trim_matches('"');
        if constant == "${LANG_ENGLISH}" {
            assert_eq!(
                words,
                "Also delete conversations, saved versions and settings"
            );
        }
        // The box is 400 x 25 px, one line.
        assert!(words.chars().count() <= 60, "{file}: {words}");
    }
}

#[test]
fn the_installer_language_files_have_no_byte_order_mark() {
    for (file, _) in language_files() {
        let bytes = std::fs::read(manifest_dir().join(&file)).unwrap();
        // The bundler adds the byte order mark when it copies the file; a second would break it.
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "{file}");
        String::from_utf8(bytes).unwrap();
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
    let write = at(r#"WriteRegStr HKCU "${MANUPRODUCTKEY}" "Installer Language" $R1"#);
    let per_machine = at(r#"!if "${INSTALLMODE}" == "perMachine""#);
    assert!(own < read && read < hindi && hindi < english && english < write);
    assert_eq!(body[hindi + 1], "StrCpy $R1 ${LANG_HINDI}");
    assert_eq!(body[english + 1], "StrCpy $R1 ${LANG_ENGLISH}");
    assert_eq!(body[english + 2], "${EndIf}");
    // Both installers: before the per-machine part, never inside it.
    assert!(write < per_machine, "{body:?}");
    // HKCU only, nothing written anywhere else.
    let writes = body.iter().filter(|line| line.starts_with("WriteReg"));
    assert_eq!(writes.count(), 1, "{body:?}");
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
