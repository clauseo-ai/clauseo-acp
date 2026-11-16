//! The installers' VERSIONINFO (the template's block `version-info`): the full set the sandbox's
//! set-up exe has (`crates/sandbox/build.rs`), after Defender's machine-learning check took the
//! per-user installer for malware with upstream's thinner set. The block's lines are read from the
//! template, kept for each install mode as `!if` keeps them, and their defines filled in as the
//! bundler fills them from `tauri.conf.json`.

use std::collections::HashMap;

use super::nsis_run::{defines, preprocess, words};
use super::sources::read;
use super::template::{TEMPLATE, block_named, split_blocks};

/// The block's code lines, comments and blank lines left out.
fn block_lines() -> Vec<String> {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    block_named(&blocks, "version-info")
        .lines
        .iter()
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .cloned()
        .collect()
}

/// The template's `!define`s, as written (`{{…}}` placeholders before the bundler fills them).
fn template_defines() -> HashMap<String, String> {
    let code: Vec<String> = read(TEMPLATE)
        .lines()
        .map(|l| l.trim().to_owned())
        .collect();
    defines(&code)
}

/// `${NAME}` in `text` from `values`; a name missing there fails.
fn fill(text: &str, values: &HashMap<&str, String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        let (name, after) = rest[at + 2..].split_once('}').expect("an unclosed ${");
        out.push_str(
            values
                .get(name)
                .unwrap_or_else(|| panic!("no value for {name}")),
        );
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The installer's version strings in `mode` (the template's `INSTALLMODE`), in block order, and
/// its `VIProductVersion`.
fn version_info(mode: &str) -> (Vec<(String, String)>, String) {
    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    let text = |pointer: &str| conf.pointer(pointer).unwrap().as_str().unwrap().to_owned();
    // What the bundler puts in the template's placeholders (tauri-bundler 2.12.2, nsis/mod.rs:
    // manufacturer is bundle.publisher, product_name productName).
    let values: HashMap<&str, String> = HashMap::from([
        ("PRODUCTNAME", text("/productName")),
        ("MANUFACTURER", text("/bundle/publisher")),
        ("COPYRIGHT", text("/bundle/copyright")),
        ("VERSION", "0.0.1".to_owned()),
        ("VERSIONWITHBUILD", "0.0.1.0".to_owned()),
    ]);
    let mut defines = template_defines();
    defines.insert("INSTALLMODE".to_owned(), mode.to_owned());
    let mut keys = Vec::new();
    let mut product = None;
    for line in preprocess(&block_lines(), &defines) {
        let parts = words(&line);
        match parts
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["VIAddVersionKey", key, value] => keys.push(((*key).to_owned(), fill(value, &values))),
            ["VIProductVersion", value] => product = Some(fill(value, &values)),
            _ => panic!("a line the version-info block shouldn't hold: {line}"),
        }
    }
    (keys, product.expect("no VIProductVersion"))
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn the_per_user_installer_version_info_is_the_full_set() {
    for mode in ["currentUser", "both"] {
        let (keys, product) = version_info(mode);
        assert_eq!(product, "0.0.1.0");
        assert_eq!(
            keys,
            pairs(&[
                ("ProductName", "Clauseo"),
                ("CompanyName", "Clauseo"),
                ("FileDescription", "Clauseo Setup"),
                (
                    "LegalCopyright",
                    "Copyright 2026 AI-CLAUSEO TECHNOLOGY PRIVATE LIMITED and contributors",
                ),
                ("FileVersion", "0.0.1"),
                ("ProductVersion", "0.0.1"),
                ("InternalName", "Clauseo Setup"),
                ("OriginalFilename", "Clauseo Setup"),
            ]),
            "{mode}"
        );
    }
}

#[test]
fn the_per_machine_installer_version_info_says_all_users() {
    let (keys, _) = version_info("perMachine");
    let description: Vec<&(String, String)> = keys
        .iter()
        .filter(|(key, _)| key == "FileDescription")
        .collect();
    assert_eq!(
        description,
        [&(
            "FileDescription".to_owned(),
            "Clauseo Setup (all users)".to_owned()
        )]
    );
    // The rest as the per-user installer's.
    let others = |keys: Vec<(String, String)>| -> Vec<(String, String)> {
        keys.into_iter()
            .filter(|(key, _)| key != "FileDescription")
            .collect()
    };
    assert_eq!(others(keys), others(version_info("currentUser").0));
}

#[test]
fn the_installer_version_info_has_the_setup_exes_keys_once_each() {
    // crates/sandbox/build.rs's strings, less the key id only the set-up exe carries; NSIS
    // refuses a key given twice.
    let build = read("../sandbox/build.rs");
    for mode in ["currentUser", "perMachine"] {
        let (keys, _) = version_info(mode);
        let mut names: Vec<&str> = keys.iter().map(|(key, _)| key.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "CompanyName",
                "FileDescription",
                "FileVersion",
                "InternalName",
                "LegalCopyright",
                "OriginalFilename",
                "ProductName",
                "ProductVersion",
            ],
            "{mode}"
        );
        for name in names {
            assert!(build.contains(&format!("(\"{name}\", ")), "{name}");
        }
    }
}

#[test]
fn the_installer_version_info_uses_only_the_templates_defines() {
    // An unknown ${NAME} would stay as literal text in the installer's properties.
    let defines = template_defines();
    let mut used = 0;
    for line in block_lines() {
        let mut rest = line.as_str();
        while let Some(at) = rest.find("${") {
            let (name, after) = rest[at + 2..].split_once('}').unwrap();
            assert!(defines.contains_key(name), "{name} in {line}");
            used += 1;
            rest = after;
        }
    }
    assert!(used >= 10, "only {used}");
    // Neither OUTFILE (the bundler's working name, nsis-output.exe) nor a hard-coded product.
    for line in block_lines() {
        assert!(
            !line.contains("OUTFILE") && !line.contains("\"Clauseo"),
            "{line}"
        );
    }
}
