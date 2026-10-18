//! Every plugin is registered inside `RustOnly` (`crates/desktop/src/rust_only.rs`), which drops the
//! script the plugin would inject into every page. The desktop crate's `src/**/*.rs` is read with
//! comments stripped (`checks::strip_comments`), and each `.plugin(` call, `Builder::plugin` or
//! `AppHandle::plugin` alike, must open with `RustOnly::new(`, whitespace and line breaks allowed.

use crate::checks::{self, violation};
use crate::violation::Violation;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const CALL: &str = ".plugin(";
const WRAPPER: &str = "RustOnly::new(";
const UNWRAPPED_FIX: &str = "register it as `.plugin(RustOnly::new(...))`: unwrapped, the plugin injects its script into every page, turning plain web features into native actions (see crates/desktop/src/rust_only.rs)";

/// The `.rs` files under `root/dir/src`, as (repo path, text), sorted by path. A missing `src`
/// gives none; an unreadable file is a violation.
pub(super) fn rust_sources(root: &Path, dir: &str) -> Result<Vec<(String, String)>, Violation> {
    let src = checks::join(dir, "src");
    let mut files = Vec::new();
    collect(&root.join(&src), &mut files).map_err(|error| checks::unreadable(&src, &error))?;
    files.sort();
    files
        .into_iter()
        .map(|file| {
            let path = checks::repo_path(root, &file);
            fs::read_to_string(&file)
                .map(|text| (path.clone(), text))
                .map_err(|error| checks::unreadable(&path, &error))
        })
        .collect()
}

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
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    Ok(())
}

/// One violation per `.plugin(` call in `text` (the file at `path`) whose argument does not open
/// with `RustOnly::new(`, at the line of the call.
pub(super) fn violations(path: &str, text: &str) -> Vec<Violation> {
    let code = checks::strip_comments(text);
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = code.get(from..).and_then(|rest| rest.find(CALL)) {
        let start = from + offset;
        let after = start + CALL.len();
        let wrapped = code
            .get(after..)
            .is_some_and(|rest| rest.trim_start().starts_with(WRAPPER));
        if !wrapped {
            let line = code
                .get(..start)
                .map_or(0, |before| before.matches('\n').count());
            found.push(violation(
                path,
                checks::line_at(line),
                "a plugin is registered without `RustOnly`",
                UNWRAPPED_FIX,
            ));
        }
        from = after;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = "crates/desktop/src/lib.rs";

    #[test]
    fn wrapped_registrations_pass_on_one_line_or_across_lines() {
        let text = "tauri::Builder::default()\n    .plugin(RustOnly::new(a::init()))\n    .plugin(\n        RustOnly::new(b::init()),\n    )\n";
        assert_eq!(violations(PATH, text), Vec::<Violation>::new());
    }

    #[test]
    fn an_unwrapped_registration_fails_at_its_line() {
        let text = "tauri::Builder::default()\n    .plugin(RustOnly::new(a::init()))\n    .plugin(b::init())\n";
        let found = violations(PATH, text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, PATH);
        assert_eq!(found[0].line, 3);
        assert_eq!(
            found[0].message,
            "a plugin is registered without `RustOnly`"
        );
    }

    #[test]
    fn a_runtime_registration_on_the_app_handle_counts_too() {
        let found = violations(PATH, "fn f(app: &AppHandle) { app.plugin(c::init()); }\n");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn comments_do_not_count() {
        let text = "//! Each `.plugin(` call needs it.\n// .plugin(b::init())\nlet x = 1;\n";
        assert_eq!(violations(PATH, text), Vec::<Violation>::new());
    }

    #[test]
    fn sources_are_the_rs_files_under_src() {
        let scratch = crate::scratch::Scratch::new("plugins-registration");
        let root: &Path = &scratch;
        let src = root.join("crates/desktop/src");
        fs::create_dir_all(src.join("nested")).expect("mkdir");
        fs::write(src.join("lib.rs"), ".plugin(a::init())\n").expect("write");
        fs::write(
            src.join("nested/more.rs"),
            ".plugin(RustOnly::new(b::init()))\n",
        )
        .expect("write");
        fs::write(src.join("notes.md"), ".plugin(c::init())\n").expect("write");
        let files = rust_sources(root, "crates/desktop").expect("readable");
        let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "crates/desktop/src/lib.rs",
                "crates/desktop/src/nested/more.rs"
            ]
        );
        let found: Vec<Violation> = files
            .iter()
            .flat_map(|(path, text)| violations(path, text))
            .collect();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "crates/desktop/src/lib.rs");
    }
}
