//! `clippy_bans_every_planted_call`: builds one scratch crate whose `clippy.toml` is the file
//! core-io asks each core crate to have (the repo's root `clippy.toml` plus
//! `xtask/core-io-bans.toml`) with every `allow-invalid` taken out, with one module per form in
//! `forms.rs`, runs the pinned clippy on it offline, and asserts that each must-fail module gets
//! a `disallowed_methods` or `disallowed_types` hit inside its own file (naming the form's item,
//! when it has one) and each must-pass module gets none.
//!
//! It also fails on a compile error in a form, and on a warning about the `clippy.toml` itself
//! other than "does not refer to a reachable" for exactly the entries of the other OS family
//! (`std::os::windows::` on unix, `std::os::unix::` on Windows). So a misspelled entry fails
//! here: a `std::os::unix` one when the test runs on unix, a `std::os::windows` one when it runs
//! on Windows, any other one everywhere.

mod forms;

use super::clippy_toml;
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;
use forms::{Form, Must};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;
use toml::{Table, Value as Toml};

/// The lints whose hits count.
const LINTS: [&str; 2] = ["clippy::disallowed_methods", "clippy::disallowed_types"];

#[test]
fn clippy_bans_every_planted_call() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let expected = clippy_toml::load(&repo).expect("the repo's clippy.toml and list load");
    let forms: Vec<Form> = forms::forms()
        .into_iter()
        .filter(|form| form.only.here())
        .collect();
    let dir = Scratch::new("core-io-clippy");
    let config = clippy_toml::render(&without_allow_invalid(&expected.table));
    write_crate(&dir, &repo, &config, &forms);

    #[expect(
        clippy::disallowed_methods,
        reason = "the test reports how long clippy took"
    )]
    let started = Instant::now();
    let target = dir.join("target");
    let cmd = Cmd::new(
        Program::Cargo,
        [
            "clippy",
            "--offline",
            "--quiet",
            "--message-format=json",
            "--target-dir",
            &target.to_string_lossy(),
        ],
    );
    let ran = process::run_quiet(&cmd, &dir).expect("cargo clippy starts");
    eprintln!(
        "clippy_bans_every_planted_call: {} forms, cargo clippy took {:.1?}",
        forms.len(),
        started.elapsed()
    );

    // The text of each hit, by the file it is in.
    let mut hits: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut unreachable = BTreeSet::new();
    let mut problems = Vec::new();
    for message in ran.stdout.lines().filter_map(compiler_message) {
        let code = message
            .pointer("/code/code")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let level = message["level"].as_str().unwrap_or_default();
        let text = message["message"].as_str().unwrap_or_default();
        let rendered = message["rendered"].as_str().unwrap_or_default();
        let files = primary_files(&message);
        if LINTS.contains(&code) {
            for file in files {
                hits.entry(file).or_default().push(text.to_owned());
            }
        } else if files.iter().any(|file| file.ends_with("clippy.toml")) {
            match unreachable_path(text) {
                Some(path) => {
                    unreachable.insert(path);
                }
                None => problems.push(rendered.to_owned()),
            }
        } else if level == "error" {
            problems.push(rendered.to_owned());
        }
    }
    assert!(
        problems.is_empty() && ran.success,
        "cargo clippy failed or warned about the config:\n{}\n{}",
        problems.join("\n"),
        ran.stderr
    );
    assert_eq!(
        unreachable,
        other_family_entries(&expected.table),
        "clippy must find every list entry but those of the other OS family"
    );
    let wrong: Vec<String> = forms
        .iter()
        .filter_map(|form| {
            let found = hits
                .get(&format!("src/{}.rs", form.name))
                .map_or(&[][..], Vec::as_slice);
            let names_item = |item: &str| {
                let quoted = format!("`{item}`");
                found.iter().any(|hit| hit.contains(&quoted))
            };
            match (form.must, &form.item) {
                (Must::Fail, _) if found.is_empty() => {
                    Some(format!("{}: no hit, but it must fail", form.name))
                }
                (Must::Fail, Some(item)) if !names_item(item) => Some(format!(
                    "{}: no hit names `{item}`, only: {}",
                    form.name,
                    found.join("; ")
                )),
                (Must::Pass, _) if !found.is_empty() => Some(format!(
                    "{}: {} hit(s), but it must pass: {}",
                    form.name,
                    found.len(),
                    found.join("; ")
                )),
                _ => None,
            }
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "clippy got these forms wrong:\n{}",
        wrong.join("\n")
    );
}

/// Writes the scratch crate into `dir`: a manifest with no dependency and its own workspace,
/// `clippy.toml` holding `config`, the repo's `rust-toolchain.toml` (so the pinned clippy
/// runs), and `src/lib.rs` declaring one module per form, each in `src/<name>.rs`.
fn write_crate(dir: &Path, repo: &Path, config: &str, forms: &[Form]) {
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"planted\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         publish = false\n\n[workspace]\n",
    )
    .expect("write Cargo.toml");
    fs::write(dir.join("clippy.toml"), config).expect("write clippy.toml");
    fs::copy(
        repo.join("rust-toolchain.toml"),
        dir.join("rust-toolchain.toml"),
    )
    .expect("copy rust-toolchain.toml");
    fs::create_dir_all(dir.join("src")).expect("create src/");
    let mut lib = vec!["#![allow(unused)]".to_owned()];
    for form in forms {
        lib.push(format!("pub mod {};", form.name));
        fs::write(
            dir.join("src").join(format!("{}.rs", form.name)),
            &form.source,
        )
        .expect("write a form");
    }
    lib.push(String::new());
    fs::write(dir.join("src/lib.rs"), lib.join("\n")).expect("write lib.rs");
}

/// The `message` of a `compiler-message` line of cargo's JSON output.
fn compiler_message(line: &str) -> Option<Value> {
    let mut parsed: Value = serde_json::from_str(line).ok()?;
    (parsed["reason"] == "compiler-message").then(|| parsed["message"].take())
}

/// The files of a message's primary spans, `/`-separated as cargo prints them relative to the
/// crate (`src/x.rs`), or absolute for a file outside it.
fn primary_files(message: &Value) -> Vec<String> {
    message["spans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|span| span["is_primary"] == true)
        .filter_map(|span| span["file_name"].as_str())
        .map(|file| file.replace('\\', "/"))
        .collect()
}

/// `table` with `allow-invalid` taken out of every entry, so clippy warns about each path it
/// cannot find.
fn without_allow_invalid(table: &Table) -> Table {
    let mut table = table.clone();
    for (_, rows) in &mut table {
        let Some(rows) = rows.as_array_mut() else {
            continue;
        };
        for row in rows.iter_mut().filter_map(Toml::as_table_mut) {
            row.remove("allow-invalid");
        }
    }
    table
}

/// The paths of `table`'s `disallowed-methods` and `disallowed-types` entries in the other OS
/// family's `std::os` module, which clippy cannot find on this host.
fn other_family_entries(table: &Table) -> BTreeSet<String> {
    let other = if cfg!(windows) {
        "std::os::unix::"
    } else {
        "std::os::windows::"
    };
    ["disallowed-methods", "disallowed-types"]
        .into_iter()
        .filter_map(|key| table.get(key).and_then(Toml::as_array))
        .flatten()
        .filter_map(|row| row.get("path").and_then(Toml::as_str))
        .filter(|path| path.starts_with(other))
        .map(str::to_owned)
        .collect()
}

/// The path in clippy's warning that a config entry names nothing it can find, the
/// `std::os::windows::fs::symlink_dir` of "`` `std::os::windows::fs::symlink_dir` does not refer
/// to a reachable function ``".
fn unreachable_path(message: &str) -> Option<String> {
    let rest = message.strip_prefix('`')?;
    let (path, tail) = rest.split_once('`')?;
    tail.starts_with(" does not refer to a reachable")
        .then(|| path.to_owned())
}
