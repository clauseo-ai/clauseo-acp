use super::DOWNLOAD_URL;
use super::dialog::{APP_ICON, CLOSE, DOWNLOAD, Next, main_icon, message_box_text, next};
use super::version::{MINIMUM, Verdict, Version, verdict};
use super::words::{EN, WIDTH, fits};
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
        "154.0.3485.54",
        "100",
        "154.999.999.999",
        "120.0.2210.91 beta",
    ] {
        assert_eq!(verdict(Some(older), minimum()), Verdict::TooOld, "{older}");
    }
}

#[test]
fn the_minimum_itself_starts() {
    assert_eq!(verdict(Some("155.0.0.0"), minimum()), Verdict::Start);
    assert_eq!(verdict(Some("155"), minimum()), Verdict::Start);
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

#[test]
fn every_string_is_there() {
    for text in EN.all() {
        assert_ne!(text.trim(), "");
    }
    assert_eq!(EN.title, "Clauseo");
}

#[test]
fn the_download_link_is_microsofts_bootstrapper_page() {
    assert_eq!(
        DOWNLOAD_URL,
        "https://go.microsoft.com/fwlink/p/?LinkId=2124703"
    );
}

/// The `LangString` keys of a language file, in order, each with its language constant.
fn lang_strings(relative: &str) -> Vec<(String, String)> {
    read(relative)
        .lines()
        .filter_map(|line| line.strip_prefix("LangString "))
        .map(|rest| {
            let mut parts = rest.splitn(3, ' ');
            let key = parts.next().unwrap().to_owned();
            let constant = parts.next().unwrap().to_owned();
            let text = parts.next().unwrap();
            assert!(
                text.starts_with('"') && text.ends_with('"'),
                "{key}: {text}"
            );
            (key, constant)
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
    assert_eq!(
        message_box_text(&EN),
        "Clauseo needs Microsoft Edge WebView2 to open\n\n\
         If this computer is your firm's, its IT team may need to allow it."
    );
}

#[test]
fn the_dialog_is_wide_enough_for_the_main_line_and_no_wider_than_a_small_screen() {
    assert!(fits(EN.instruction, WIDTH), "{WIDTH}");
    assert!(!fits(EN.instruction, 200));
    // 150 % on a 1366 px wide screen: about 1.75 px a dialog unit at 100 %.
    assert!(f64::from(WIDTH) * 1.75 * 1.5 < 1366.0);
}

#[test]
fn the_installer_english_file_names_the_product_through_nsis() {
    let text = read(&english_file());
    // Language files are included, not rendered, so a template mark would show as it is.
    for line in text.lines().filter(|line| !line.starts_with(';')) {
        assert!(!line.contains("{{"), "{line}");
    }
    for key in ["appRunning", "appRunningOkKill", "failedToKillApp"] {
        let prefix = format!("LangString {key} ${{LANG_ENGLISH}} ");
        let line = text
            .lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .unwrap();
        assert!(line.contains("${PRODUCTNAME}"), "{key}: {line}");
    }
}

/// The installer's English file, as `customLanguageFiles` names it.
fn english_file() -> String {
    let conf = tauri_conf();
    let nsis = &conf["bundle"]["windows"]["nsis"];
    assert_eq!(nsis["languages"], serde_json::json!(["English"]));
    let files = nsis["customLanguageFiles"].as_object().unwrap();
    assert_eq!(files.keys().collect::<Vec<_>>(), ["English"]);
    files["English"].as_str().unwrap().to_owned()
}

#[test]
fn the_installer_english_file_holds_tauris_27_messages_and_clauseos_4_once_each() {
    // Tauri's 27, and the four lines windows/hooks.nsh shows (deleteAppDataFailed,
    // deleteAppDataKept, deleteAppDataKeptFailed and deleteAppDataFailedUnnamed), which a test
    // there holds to the hook.
    let strings = lang_strings(&english_file());
    assert_eq!(strings.len(), 31);
    let mut keys: Vec<&str> = strings.iter().map(|(key, _)| key.as_str()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), 31);
    assert!(
        strings
            .iter()
            .all(|(_, constant)| constant == "${LANG_ENGLISH}")
    );
}

#[test]
fn the_delete_app_data_box_says_clauseos_words_on_one_line() {
    let text = read(&english_file());
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("LangString deleteAppData ${LANG_ENGLISH} "))
        .unwrap();
    let words = line.trim_matches('"');
    assert_eq!(
        words,
        "Also delete conversations, saved versions and settings"
    );
    // The box is 400 x 25 px, one line.
    assert!(words.chars().count() <= 60, "{words}");
}

#[test]
fn the_installer_english_file_has_no_byte_order_mark() {
    let bytes = std::fs::read(manifest_dir().join(english_file())).unwrap();
    // The bundler adds the byte order mark when it copies the file; a second would break it.
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    String::from_utf8(bytes).unwrap();
}
