//! `python-runtime/THIRD-PARTY-NOTICES.txt`: every licence text of what the archives ship
//! (`CPython`, the libraries PBS builds into it, pip, and each package's wheel), written by
//! `bump` from the files themselves and stamped with the lock's sha256, so `check` sees a stale
//! file offline and `build` checks that every licence file in the runtime is in it before it
//! ships it.

use super::Failure;
use super::lock::Lock;
use super::net::hex;
use super::target::TARGETS;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// The line that carries the lock's stamp.
const STAMP: &str = "lock-sha256: ";

/// The PBS extension modules whose licences stay out: `build` removes them.
pub(crate) const REMOVED_MODULES: [&str; 1] = ["_tkinter"];

/// The lock's stamp: the sha256 of its text with `\r\n` read as `\n`, so a Windows checkout
/// stamps the same.
pub(crate) fn stamp(lock_text: &str) -> String {
    hex(&Sha256::digest(lock_text.replace("\r\n", "\n").as_bytes()))
}

/// A licence text, the path it was read from (within its package), and the targets it ships on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Text {
    pub path: String,
    pub body: String,
    pub targets: BTreeSet<&'static str>,
}

/// One component of the runtime and its licence texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Section {
    pub title: String,
    pub licence: String,
    pub note: String,
    pub texts: Vec<Text>,
}

impl Section {
    pub(crate) fn new(title: String, licence: String, note: String) -> Section {
        Section {
            title,
            licence,
            note,
            texts: Vec::new(),
        }
    }

    /// Adds `body`, read from `path` on `target`: to the text with the same path and body when
    /// there is one, else as a new text.
    pub(crate) fn add(&mut self, path: &str, body: &str, target: &'static str) {
        let body = normalize(body);
        if let Some(text) = self
            .texts
            .iter_mut()
            .find(|text| text.path == path && text.body == body)
        {
            text.targets.insert(target);
        } else {
            self.texts.push(Text {
                path: path.to_owned(),
                body,
                targets: BTreeSet::from([target]),
            });
        }
    }
}

/// `body` with `\r\n` read as `\n` and no trailing blank space.
pub(crate) fn normalize(body: &str) -> String {
    body.replace("\r\n", "\n").trim_end().to_owned()
}

/// The targets as the notices name them: `all targets`, or the triples.
fn targets_text(targets: &BTreeSet<&'static str>) -> String {
    if targets.len() == TARGETS.len() {
        "all targets".to_owned()
    } else {
        targets.iter().copied().collect::<Vec<_>>().join(", ")
    }
}

/// A rule line.
const RULE: &str =
    "================================================================================";
const THIN: &str =
    "--------------------------------------------------------------------------------";

/// The notices file for `lock` (whose text is `lock_text`) and `sections`, in their order.
pub(crate) fn render(lock: &Lock, lock_text: &str, sections: &[Section]) -> String {
    let mut parts = vec![
        "Third-party notices for Clauseo's Python runtime".to_owned(),
        RULE.to_owned(),
        String::new(),
        "Written by `cargo xtask python-runtime bump` from python-runtime/lock.toml; do not edit."
            .to_owned(),
        format!("{STAMP}{}", stamp(lock_text)),
        String::new(),
        format!(
            "The runtime is CPython {} as built by python-build-standalone (release {}), with the",
            lock.runtime.python, lock.runtime.pbs_release
        ),
        "Python packages below. Tcl/Tk, tkinter, IDLE and turtle are removed from the runtime, so"
            .to_owned(),
        "their notices are not included.".to_owned(),
        String::new(),
        "Contents".to_owned(),
    ];
    parts.extend(
        sections
            .iter()
            .map(|section| format!("- {} ({})", section.title, section.licence)),
    );
    for section in sections {
        parts.extend([
            String::new(),
            RULE.to_owned(),
            format!("{} ({})", section.title, section.licence),
        ]);
        if !section.note.is_empty() {
            parts.push(section.note.clone());
        }
        parts.push(RULE.to_owned());
        for text in &section.texts {
            parts.extend([
                String::new(),
                THIN.to_owned(),
                format!("{} ({})", text.path, targets_text(&text.targets)),
                THIN.to_owned(),
                text.body.clone(),
            ]);
        }
    }
    parts.push(String::new());
    parts.join("\n")
}

/// The section title of a package: `numpy 2.5.4`.
pub(crate) fn package_title(name: &str, version: &str) -> String {
    format!("{name} {version}")
}

/// Every problem with `notices` for `lock`: a stamp that is not the lock's, or a package of the
/// lock it does not list.
pub(crate) fn problems(notices: &str, lock: &Lock, lock_text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let want = format!("{STAMP}{}", stamp(lock_text));
    if !notices.lines().any(|line| line == want) {
        found.push(
            "THIRD-PARTY-NOTICES.txt was not written from this lock (its lock-sha256 differs)"
                .to_owned(),
        );
    }
    for package in &lock.packages {
        let line = format!(
            "- {} ({})",
            package_title(&package.name, &package.version),
            package.licence
        );
        if !notices.lines().any(|l| l == line) {
            found.push(format!("THIRD-PARTY-NOTICES.txt does not list `{line}`"));
        }
    }
    found
}

/// The licence files of a `.dist-info` folder: everything under `licenses/` (PEP 639), and the
/// files at its top whose names start with LICENSE, LICENCE, COPYING, NOTICE or AUTHORS. Paths are
/// relative to the folder's parent (`numpy-2.5.4.dist-info/LICENSE.txt`), sorted.
pub(crate) fn dist_info_texts(dist_info: &Path) -> Result<Vec<(String, String)>, Failure> {
    let folder = dist_info
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut found = Vec::new();
    for (rel, kind, _) in super::tree::walk(dist_info)? {
        if kind != super::tree::Kind::File {
            continue;
        }
        let top = rel
            .split('/')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let licence_like = ["LICENSE", "LICENCE", "COPYING", "NOTICE", "AUTHORS"]
            .iter()
            .any(|prefix| top.starts_with(prefix));
        if rel.starts_with("licenses/") || (!rel.contains('/') && licence_like) {
            let bytes = fs::read(dist_info.join(&rel))
                .map_err(|error| Failure::io(&format!("reading {rel}"), &error))?;
            found.push((
                format!("{folder}/{rel}"),
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
        }
    }
    Ok(found)
}

/// The licence files PBS's `PYTHON.json` names for `CPython` and the libraries built into it, each
/// with the modules that use it and their SPDX ids, leaving out `REMOVED_MODULES`.
pub(crate) fn pbs_components(
    python_json: &serde_json::Value,
) -> BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> {
    let mut found: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    let strings = |value: Option<&serde_json::Value>| -> Vec<String> {
        value
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect()
    };
    if let Some(path) = python_json.get("license_path").and_then(|p| p.as_str()) {
        let entry = found.entry(path.to_owned()).or_default();
        entry.0.insert("CPython".to_owned());
        entry.1.extend(strings(python_json.get("licenses")));
    }
    let extensions = python_json
        .get("build_info")
        .and_then(|info| info.get("extensions"))
        .and_then(serde_json::Value::as_object);
    for (module, variants) in extensions.into_iter().flatten() {
        if REMOVED_MODULES.contains(&module.as_str()) {
            continue;
        }
        for variant in variants.as_array().into_iter().flatten() {
            let ids = strings(variant.get("licenses"));
            for path in strings(variant.get("license_paths")) {
                let entry = found.entry(path).or_default();
                entry.0.insert(module.clone());
                entry.1.extend(ids.iter().cloned());
            }
        }
    }
    found
}

/// Whether `notices` holds `body` (normalized) word for word.
pub(crate) fn covers(notices: &str, body: &str) -> bool {
    notices.contains(&normalize(body))
}
