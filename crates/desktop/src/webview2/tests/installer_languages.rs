//! The installer's language files (English.nsh, Hindi.nsh): where they sit, the product named
//! through NSIS, every key once and in the same order, the app-running lines in Clauseo's words,
//! Hindi in the Hindi file, the delete-app-data box on one line, and no byte order mark.

use super::{manifest_dir, read, tauri_conf};

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
fn the_installer_language_files_hold_tauris_24_messages_and_clauseos_32_once_each() {
    // Tauri's 24, the three lines of Tauri's CheckIfAppIsRunning that Clauseo words itself
    // (appRunning, appRunningOkKill and failedToKillApp), the eight lines windows/hooks.nsh shows
    // after the app deletes its data or about S15's removal (deleteAppDataFailed,
    // deleteAppDataKept, deleteAppDataKeptFailed, deleteAppDataFailedUnnamed, removeStillRunning,
    // removeSkippedSilent, removeExeMissing and removeAdminCopyRefused), which tests there hold to
    // the hook, its two about the firm-policy
    // folder (policyFolderExists and policyFolderFailed), the installers' setupExeRemoved, its
    // twelve about the admin copy (adminCopyAsk, adminCopyWorking, adminCopyDone,
    // adminCopyDeclined, adminCopyBlocked, adminCopyFailed, adminCopyOtherAccount,
    // adminCopyMachineFailed, adminCopyNewerKept, adminCopyAppRunning, adminCopyStillRunning and
    // adminCopySkippedSilent),
    // the per-machine installer's and uninstaller's about their temporary folders
    // (installTempFailed and uninstallTempFailed), the uninstaller's when it isn't started in
    // place (uninstallNotInPlace), the per-machine installer's details line on an ARM64
    // computer (nativeArm64Notice), the details line about an old WebView2 (webview2TooOld),
    // and both installers' when Clauseo.exe is gone after the files are placed
    // (clauseoAppRemovedByAv).
    let [english, hindi] = language_files();
    let mut orders = Vec::new();
    for (file, constant) in [english, hindi] {
        let strings = lang_strings(&file);
        assert_eq!(strings.len(), 56, "{file}");
        let order: Vec<String> = strings.iter().map(|(key, _, _)| key.clone()).collect();
        let mut keys = order.clone();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 56, "{file}");
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
fn the_app_running_lines_are_clauseos_words_above_tauris_stock_ones() {
    // Tauri's CheckIfAppIsRunning says "is running! Click OK to kill it"; Clauseo's say what
    // happens, with no exclamation mark and no kill. They sit above Tauri's stock lines, so a
    // Tauri upgrade that copies its lines back brings a second key, which the count test fails.
    for (file, constant) in language_files() {
        let text = read(&file);
        let stock = text
            .find("; Tauri's stock English")
            .unwrap_or_else(|| panic!("{file}: no stock block"));
        for key in ["appRunning", "appRunningOkKill", "failedToKillApp"] {
            let prefix = format!("LangString {key} {constant} ");
            let at = text
                .find(prefix.as_str())
                .unwrap_or_else(|| panic!("{file}: no {key}"));
            assert!(at < stock, "{file}: {key} is among Tauri's stock lines");
            let line = text[at..].lines().next().unwrap();
            assert!(!line.contains('!'), "{file}: {line}");
            assert!(!line.to_lowercase().contains("kill it"), "{file}: {line}");
        }
    }
    let english = read("windows/nsis/English.nsh");
    let words = |key: &str| {
        let prefix = format!("LangString {key} ${{LANG_ENGLISH}} ");
        english
            .lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .unwrap()
            .to_owned()
    };
    // The box has OK and Cancel: it names both, then, after a blank line (`$\n$\n`, as the
    // other Clauseo boxes break), what is kept and what is lost.
    assert_eq!(
        words("appRunningOkKill"),
        "\"${PRODUCTNAME} is open. Click OK to close it and continue, or Cancel to leave it open \
         and stop here.$\\n$\\nYour conversations are kept. An answer still being written stops, \
         and your question and the answer so far are kept. A question you've typed but not sent \
         is lost, so copy it before you click OK.\""
    );
    assert_eq!(
        words("failedToKillApp"),
        "\"Stopped, because ${PRODUCTNAME} didn't close. Quit ${PRODUCTNAME}, then try again.\""
    );
    for key in ["appRunning", "failedToKillApp"] {
        assert!(
            words(key).contains("Quit ${PRODUCTNAME}, then try again."),
            "{key}"
        );
    }
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
