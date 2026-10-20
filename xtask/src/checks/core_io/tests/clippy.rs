//! `clippy_bans_every_planted_call`: builds two scratch crates whose `clippy.toml` is the file
//! core-io asks each core crate to have (the repo's root `clippy.toml` plus
//! `xtask/core-io-bans.toml`) with every `allow-invalid` taken out, and whose root carries the
//! forbid line core-io asks every target root of a held crate to carry. The first holds one module
//! per form in `forms.rs`, the second one per form of `forms::forbidden()`. A third crate has the
//! root `clippy.toml` alone, and one module per form of `forms::root()`: the doors every crate
//! has, and the reads they leave allowed. It runs the pinned clippy on each offline, with the
//! clippy step's flags (`CLIPPY_FLAGS`), and asserts that each must-fail module gets a
//! `disallowed_methods` or `disallowed_types` hit, or error E0453, inside its own file (naming
//! the form's item, when it has one, on the form's line and with its note, when it has them) and
//! each must-pass module gets none.
//!
//! Both crates' manifests set the two lints to `allow` in `[lints.clippy]`, and both runs pass
//! `-A` of the two lints after `--`, which cargo hands to clippy-driver in `CLIPPY_ARGS`. The
//! first run's must-fail forms still fail, so neither undoes the forbid line.
//!
//! It also fails on any other compile error, and on a warning about the `clippy.toml` itself
//! other than "does not refer to a reachable" for exactly the entries of the other OS family
//! (`std::os::windows::` on unix, `std::os::unix::` on Windows). So a misspelled entry fails
//! here: a `std::os::unix` one when the test runs on unix, a `std::os::windows` one when it runs
//! on Windows, any other one everywhere.

mod forms;

use super::clippy_toml;
use crate::checks::core_io::roots::forbid_line;
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;
use crate::steps::plan::CLIPPY_FLAGS;
use forms::{Form, Must};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;
use toml::{Table, Value as Toml};

/// The lints whose hits count.
const LINTS: [&str; 2] = ["clippy::disallowed_methods", "clippy::disallowed_types"];

/// The error an `allow` or `expect` of a forbidden lint gets, which counts as a hit too.
const FORBID_ERROR: &str = "E0453";

/// One hit: where it is and what it says.
struct Hit {
    /// The line of its primary span.
    line: u64,
    /// The message: "use of a disallowed method `<path>`".
    text: String,
    /// Its notes and help (the entry's reason) and suggested replacements, joined.
    notes: String,
}

/// What one clippy run reported.
struct Run {
    /// Each hit, by the file it is in.
    hits: BTreeMap<String, Vec<Hit>>,
    /// The paths of the config entries clippy could not find.
    unreachable: BTreeSet<String>,
    /// Every other error, and every other warning about the `clippy.toml`, as rendered.
    problems: Vec<String>,
    stderr: String,
}

#[test]
fn clippy_bans_every_planted_call() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let expected = clippy_toml::load(&repo).expect("the repo's clippy.toml and list load");
    let here = |forms: Vec<Form>| -> Vec<Form> {
        forms.into_iter().filter(|form| form.only.here()).collect()
    };
    let forms = here(forms::forms());
    let forbidden = here(forms::forbidden());
    let dir = Scratch::new("core-io-clippy");
    let config = clippy_toml::render(&without_allow_invalid(&expected.table));
    let target = dir.join("target");
    let bans = dir.join("bans");
    write_crate(&bans, &repo, &config, &forms);
    let run = clippy(&bans, &target, forms.len());
    let forbid = dir.join("forbid");
    write_crate(&forbid, &repo, &config, &forbidden);
    let forbid_run = clippy(&forbid, &target, forbidden.len());
    let root_forms = here(forms::root());
    let root_table: Table = toml::from_str(
        &fs::read_to_string(repo.join(clippy_toml::ROOT)).expect("read the root clippy.toml"),
    )
    .expect("the root clippy.toml parses");
    let root_config =
        toml::to_string(&without_allow_invalid(&root_table)).expect("the root table renders");
    let root = dir.join("root");
    write_crate(&root, &repo, &root_config, &root_forms);
    let root_run = clippy(&root, &target, root_forms.len());

    for run in [&run, &forbid_run, &root_run] {
        assert!(
            run.problems.is_empty(),
            "cargo clippy failed or warned about the config:\n{}\n{}",
            run.problems.join("\n"),
            run.stderr
        );
    }
    assert_eq!(
        run.unreachable,
        other_family_entries(&expected.table),
        "clippy must find every list entry but those of the other OS family"
    );
    assert_eq!(
        root_run.unreachable,
        other_family_entries(&root_table),
        "clippy must find every root entry but those of the other OS family"
    );
    let wrong: Vec<String> = wrong_forms(&forms, &run)
        .into_iter()
        .chain(wrong_forms(&forbidden, &forbid_run))
        .chain(wrong_forms(&root_forms, &root_run))
        .collect();
    assert!(
        wrong.is_empty(),
        "clippy got these forms wrong:\n{}\n{}\n{}\n{}",
        wrong.join("\n"),
        run.stderr,
        forbid_run.stderr,
        root_run.stderr
    );
}

/// Runs the pinned clippy offline on the scratch crate in `dir`, which holds `count` forms, with
/// the clippy step's flags and `-A` of the two lints after `--`, and reads what it reported.
fn clippy(dir: &Path, target: &Path, count: usize) -> Run {
    #[expect(
        clippy::disallowed_methods,
        reason = "the test reports how long clippy took"
    )]
    let started = Instant::now();
    let target = target.to_string_lossy();
    let args = ["clippy", "--offline", "--quiet"]
        .into_iter()
        .chain(CLIPPY_FLAGS)
        .chain([
            "--target-dir",
            &target,
            "--",
            "-A",
            LINTS[0],
            "-A",
            LINTS[1],
        ]);
    let cmd = Cmd::new(Program::Cargo, args);
    let ran = process::run_quiet(&cmd, dir).expect("cargo clippy starts");
    eprintln!(
        "clippy_bans_every_planted_call: {count} forms in {}, cargo clippy took {:.1?}",
        dir.display(),
        started.elapsed()
    );
    let mut run = Run {
        hits: BTreeMap::new(),
        unreachable: BTreeSet::new(),
        problems: Vec::new(),
        stderr: ran.stderr,
    };
    for message in ran.stdout.lines().filter_map(compiler_message) {
        let code = message
            .pointer("/code/code")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let level = message["level"].as_str().unwrap_or_default();
        let text = message["message"].as_str().unwrap_or_default();
        let rendered = message["rendered"].as_str().unwrap_or_default();
        let spans = primary_spans(&message);
        if LINTS.contains(&code) || code == FORBID_ERROR {
            let notes = notes(&message);
            for (file, line) in spans {
                run.hits.entry(file).or_default().push(Hit {
                    line,
                    text: text.to_owned(),
                    notes: notes.clone(),
                });
            }
        } else if spans.iter().any(|(file, _)| file.ends_with("clippy.toml")) {
            match unreachable_path(text) {
                Some(path) => {
                    run.unreachable.insert(path);
                }
                None => run.problems.push(rendered.to_owned()),
            }
        } else if level == "error" {
            run.problems.push(rendered.to_owned());
        }
    }
    run
}

/// What `run` got wrong about `forms`: a must-fail form without a hit in its file, or without
/// one naming its item (on its line, with its note, when the form has them), and a must-pass
/// form with one.
fn wrong_forms(forms: &[Form], run: &Run) -> Vec<String> {
    forms
        .iter()
        .filter_map(|form| {
            let found = run
                .hits
                .get(&format!("src/{}.rs", form.name))
                .map_or(&[][..], Vec::as_slice);
            let listed = || {
                let hits: Vec<String> = found
                    .iter()
                    .map(|hit| format!("line {}: {} ({})", hit.line, hit.text, hit.notes))
                    .collect();
                hits.join("; ")
            };
            let matches = |item: &str, hit: &Hit| {
                hit.text.contains(&format!("`{item}`"))
                    && form.line.is_none_or(|line| hit.line == line)
                    && form
                        .note
                        .as_ref()
                        .is_none_or(|note| hit.notes.contains(note))
            };
            match (form.must, &form.item) {
                (Must::Fail, _) if found.is_empty() => {
                    Some(format!("{}: no hit, but it must fail", form.name))
                }
                (Must::Fail, Some(item)) if !found.iter().any(|hit| matches(item, hit)) => {
                    Some(format!(
                        "{}: no hit names `{item}`{}{}, only: {}",
                        form.name,
                        form.line
                            .map(|line| format!(" on line {line}"))
                            .unwrap_or_default(),
                        form.note
                            .as_ref()
                            .map(|note| format!(" with \"{note}\""))
                            .unwrap_or_default(),
                        listed()
                    ))
                }
                (Must::Pass, _) if !found.is_empty() => Some(format!(
                    "{}: {} hit(s), but it must pass: {}",
                    form.name,
                    found.len(),
                    listed()
                )),
                _ => None,
            }
        })
        .collect()
}

/// Writes a scratch crate into `dir`: a manifest with no dependency, its own workspace, a feature
/// `io` that is off by default and the two lints set to `allow`, `clippy.toml` holding
/// `config`, the repo's `rust-toolchain.toml`, so the pinned clippy runs, and `src/lib.rs`,
/// which starts with `forbid_line()` and declares one module per form, each in `src/<name>.rs`.
fn write_crate(dir: &Path, repo: &Path, config: &str, forms: &[Form]) {
    Scratch::create_dir_all(dir.join("src")).expect("create src/");
    Scratch::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"planted\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         publish = false\n\n[features]\nio = []\n\n[lints.clippy]\ndisallowed_methods = \"allow\"\n\
         disallowed_types = \"allow\"\n\n[workspace]\n",
    )
    .expect("write Cargo.toml");
    Scratch::write(dir.join("clippy.toml"), config).expect("write clippy.toml");
    Scratch::copy(
        repo.join("rust-toolchain.toml"),
        dir.join("rust-toolchain.toml"),
    )
    .expect("copy rust-toolchain.toml");
    let mut lib = vec![forbid_line(), "#![allow(unused)]".to_owned()];
    for form in forms {
        lib.push(format!("pub mod {};", form.name));
        Scratch::write(
            dir.join("src").join(format!("{}.rs", form.name)),
            &form.source,
        )
        .expect("write a form");
    }
    lib.push(String::new());
    Scratch::write(dir.join("src/lib.rs"), lib.join("\n")).expect("write lib.rs");
}

/// The `message` of a `compiler-message` line of cargo's JSON output.
fn compiler_message(line: &str) -> Option<Value> {
    let mut parsed: Value = serde_json::from_str(line).ok()?;
    (parsed["reason"] == "compiler-message").then(|| parsed["message"].take())
}

/// The file and first line of a message's primary spans, the file `/`-separated as cargo prints
/// it relative to the crate (`src/x.rs`), or absolute for a file outside it.
fn primary_spans(message: &Value) -> Vec<(String, u64)> {
    message["spans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|span| span["is_primary"] == true)
        .filter_map(|span| {
            let file = span["file_name"].as_str()?.replace('\\', "/");
            Some((file, span["line_start"].as_u64().unwrap_or_default()))
        })
        .collect()
}

/// A message's children (its notes and help, where clippy puts an entry's reason) and the
/// replacements they suggest, joined by ` | `.
fn notes(message: &Value) -> String {
    let children = message["children"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let mut parts = Vec::new();
    for child in children {
        parts.extend(child["message"].as_str().map(str::to_owned));
        let spans = child["spans"].as_array().map_or(&[][..], Vec::as_slice);
        parts.extend(
            spans
                .iter()
                .filter_map(|span| span["suggested_replacement"].as_str())
                .map(str::to_owned),
        );
    }
    parts.join(" | ")
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
