//! `clippy_bans_every_planted_call`: builds one scratch crate whose `clippy.toml` is the file
//! core-io asks each core crate to have (the repo's root `clippy.toml` plus
//! `xtask/core-io-bans.toml`), with one module per form in `forms.rs`, runs the pinned clippy on
//! it offline, and asserts that each must-fail module gets a `disallowed_methods` or
//! `disallowed_types` hit inside its own file and each must-pass module gets none. It also fails
//! on a compile error in a form and on a warning about the `clippy.toml` itself (a path that
//! does not resolve), so a misspelled entry in the list fails here.

mod forms;

use super::clippy_toml;
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;
use forms::{Form, Must};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

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
    write_crate(&dir, &repo, &expected.text, &forms);

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

    let mut hits: BTreeMap<String, usize> = BTreeMap::new();
    let mut problems = Vec::new();
    for message in ran.stdout.lines().filter_map(compiler_message) {
        let code = message
            .pointer("/code/code")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let level = message["level"].as_str().unwrap_or_default();
        let rendered = message["rendered"].as_str().unwrap_or_default();
        let files = primary_files(&message);
        if LINTS.contains(&code) {
            for file in files {
                *hits.entry(file).or_default() += 1;
            }
        } else if level == "error" || files.iter().any(|file| file.ends_with("clippy.toml")) {
            problems.push(rendered.to_owned());
        }
    }
    assert!(
        problems.is_empty() && ran.success,
        "cargo clippy failed or warned about the config:\n{}\n{}",
        problems.join("\n"),
        ran.stderr
    );
    let wrong: Vec<String> = forms
        .iter()
        .filter_map(|form| {
            let found = hits
                .get(&format!("src/{}.rs", form.name))
                .copied()
                .unwrap_or(0);
            match (form.must, found) {
                (Must::Fail, 0) => Some(format!("{}: no hit, but it must fail", form.name)),
                (Must::Pass, n) if n > 0 => {
                    Some(format!("{}: {n} hit(s), but it must pass", form.name))
                }
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
