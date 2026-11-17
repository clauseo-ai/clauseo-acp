//! The 21 installer lines that were Tauri's stock English, now in Clauseo's words: the reinstall
//! page's and the `WebView2` section's (`English.nsh` says where each shows). Each sits once in
//! each language file, above Tauri's stock lines, keeps the NSIS registers its moment fills, and
//! is listed in the running copy (`ui/app/doc-approvals.json`).

use super::read;

/// The keys, in the files' order.
const REWORDED: [&str; 21] = [
    "alreadyInstalled",
    "chooseMaintenanceOption",
    "choowHowToInstall",
    "alreadyInstalledLong",
    "addOrReinstall",
    "uninstallApp",
    "olderOrUnknownVersionInstalled",
    "uninstallBeforeInstalling",
    "dontUninstall",
    "dontUninstallDowngrade",
    "newerVersionInstalled",
    "unableToUninstall",
    "silentDowngrades",
    "createDesktop",
    "webview2Downloading",
    "webview2DownloadSuccess",
    "webview2DownloadError",
    "installingWebview2",
    "webview2InstallSuccess",
    "webview2InstallError",
    "webview2AbortError",
];

const FILES: [(&str, &str); 2] = [
    ("windows/nsis/English.nsh", "${LANG_ENGLISH}"),
    ("windows/nsis/Hindi.nsh", "${LANG_HINDI}"),
];

/// Each `LangString` line of `key` in `text`, with its byte offset and its words (inside the
/// quotes).
fn lines_of<'a>(text: &'a str, key: &str, constant: &str) -> Vec<(usize, &'a str)> {
    let prefix = format!("LangString {key} {constant} \"");
    let mut at = 0;
    let mut found = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(rest) = line.trim_end().strip_prefix(prefix.as_str()) {
            found.push((at, rest.strip_suffix('"').unwrap_or(rest)));
        }
        at += line.len();
    }
    found
}

fn words(file: &str, constant: &str, key: &str) -> String {
    let text = read(file);
    let found = lines_of(&text, key, constant);
    assert_eq!(found.len(), 1, "{file}: {key} once");
    found[0].1.to_owned()
}

#[test]
fn each_reworded_line_is_clauseos_once_in_each_file_above_tauris_stock_lines() {
    for (file, constant) in FILES {
        let text = read(file);
        let stock = text
            .find("; Tauri's stock English")
            .unwrap_or_else(|| panic!("{file}: no stock block"));
        let mut last = 0;
        for key in REWORDED {
            let found = lines_of(&text, key, constant);
            assert_eq!(found.len(), 1, "{file}: {key} once");
            let (at, words) = found[0];
            assert!(at < stock, "{file}: {key} is among Tauri's stock lines");
            assert!(at > last, "{file}: {key} out of order");
            last = at;
            assert!(!words.contains('!'), "{file} {key}: {words}");
            assert!(!words.contains("..."), "{file} {key}: {words}");
        }
        // The two words that fill $R4 stay Tauri's.
        for key in ["older", "unknown"] {
            let found = lines_of(&text, key, constant);
            assert_eq!(found.len(), 1, "{file}: {key} once");
            assert!(found[0].0 > stock, "{file}: {key} left Tauri's stock lines");
        }
    }
}

#[test]
fn the_reworded_lines_say_the_settled_english() {
    let english = |key| words(FILES[0].0, FILES[0].1, key);
    assert_eq!(
        english("alreadyInstalled"),
        "${PRODUCTNAME} is already installed"
    );
    assert_eq!(
        english("uninstallApp"),
        "Uninstall ${PRODUCTNAME} first, then install it again"
    );
    assert_eq!(
        english("uninstallBeforeInstalling"),
        "Uninstall the other version first"
    );
    assert_eq!(
        english("webview2AbortError"),
        "${PRODUCTNAME} needs WebView2, which couldn't be installed. Check that you're online, \
         then try again."
    );
}

#[test]
fn the_reworded_lines_keep_the_registers_their_moment_fills() {
    // installer.nsi fills $R4 (older or unknown), $0 (the download's result) and $1 (the WebView2
    // installer's exit code) before it shows these; the silent console line ends in a line break.
    let needs: [(&str, &[&str]); 5] = [
        ("alreadyInstalledLong", &["${PRODUCTNAME}", "${VERSION}"]),
        ("olderOrUnknownVersionInstalled", &["${PRODUCTNAME}", "$R4"]),
        ("webview2DownloadError", &["$0"]),
        ("webview2InstallError", &["$1"]),
        ("silentDowngrades", &["${PRODUCTNAME}"]),
    ];
    for (file, constant) in FILES {
        for (key, registers) in needs {
            let text = words(file, constant, key);
            for register in registers {
                assert!(text.contains(register), "{file} {key}: no {register}");
            }
        }
        assert!(
            words(file, constant, "silentDowngrades").ends_with("$\\n"),
            "{file}: silentDowngrades ends its console line"
        );
    }
}

#[test]
fn the_running_copy_lists_the_reworded_lines() {
    let approvals: serde_json::Value =
        serde_json::from_str(&read("../../ui/app/doc-approvals.json")).unwrap();
    let lines = approvals["crates/desktop/windows/nsis/English.nsh"]["lines"]
        .as_array()
        .unwrap();
    for key in REWORDED {
        assert!(lines.iter().any(|line| line == key), "{key}");
    }
}
