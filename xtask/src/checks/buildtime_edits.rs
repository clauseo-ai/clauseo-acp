//! The `buildtime-edits` check: no edit that `cargo xtask buildtime web` makes for its hot-reload
//! probe is left in the UI sources.
//!
//! The probe writes `data-buildtime="N"` into a file under `ui/app/src` and restores the file when
//! it ends. A run killed half-way (SIGKILL, a closed terminal) leaves the attribute behind until
//! the next web run, and it is easy to commit. Every file under `ui/app/src` is read, bytes that
//! are not UTF-8 replaced, and each line holding `data-buildtime=` fails. A missing `ui/app/src`
//! passes.

use crate::checks::{self, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The UI sources the probe edits, relative to the repo root.
const SRC: &str = "ui/app/src";

/// The text the probe writes.
const MARKER: &str = "data-buildtime=";

const MESSAGE: &str = "an interrupted `cargo xtask buildtime web` left its edit here";
const FIX: &str = "run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, or remove the attribute by hand";

/// Fails on every line under `ui/app/src` that holds the probe's attribute.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "buildtime-edits",
    run,
};

fn run(ctx: &Context) -> Vec<Violation> {
    let mut files = Vec::new();
    if let Err(error) = collect(&ctx.root.join(SRC), &mut files) {
        return vec![checks::unreadable(SRC, &error)];
    }
    files.sort();
    let mut found = Vec::new();
    for file in &files {
        let path = checks::repo_path(&ctx.root, file);
        match fs::read(file) {
            Ok(bytes) => found.extend(scan(&path, &String::from_utf8_lossy(&bytes))),
            Err(error) => found.push(checks::unreadable(&path, &error)),
        }
    }
    found
}

/// Every file under `dir`, recursively. A missing `dir` adds none.
fn collect(dir: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

/// One violation for each line of `text`, the file at `path`, that holds the probe's attribute.
fn scan(path: &str, text: &str) -> Vec<Violation> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(MARKER))
        .map(|(index, _)| checks::violation(path, checks::line_at(index), MESSAGE, FIX))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::tests::desktop_context;
    use crate::scratch::Scratch;

    #[test]
    fn a_left_attribute_fails_at_each_line_in_any_nested_file() {
        let dir = Scratch::new("buildtime-edits-left");
        fs::create_dir_all(dir.join("ui/app/src/views")).expect("create the src dirs");
        fs::write(dir.join("ui/app/src/App.tsx"), "<main>\n</main>\n").expect("write App.tsx");
        let view = "export function V() {\n  return <div data-buildtime=\"3\">x</div>;\n}\n";
        fs::write(dir.join("ui/app/src/views/V.tsx"), view).expect("write V.tsx");
        fs::write(
            dir.join("ui/app/src/logo.bin"),
            b"\xff\xfe data-buildtime=\"1\"",
        )
        .expect("write logo.bin");
        assert_eq!(
            run(&desktop_context(&dir)),
            vec![
                checks::violation("ui/app/src/logo.bin", 1, MESSAGE, FIX),
                checks::violation("ui/app/src/views/V.tsx", 2, MESSAGE, FIX),
            ]
        );
    }

    #[test]
    fn clean_sources_and_a_missing_src_pass() {
        let dir = Scratch::new("buildtime-edits-clean");
        assert_eq!(run(&desktop_context(&dir)), Vec::new());
        fs::create_dir_all(dir.join("ui/app/src")).expect("create the src dir");
        fs::write(dir.join("ui/app/src/App.tsx"), "<main data-test=\"1\" />\n")
            .expect("write App.tsx");
        assert_eq!(run(&desktop_context(&dir)), Vec::new());
    }

    #[test]
    fn the_check_is_registered_last() {
        assert_eq!(
            crate::checks::ALL.last().map(|check| check.name),
            Some("buildtime-edits")
        );
    }
}
