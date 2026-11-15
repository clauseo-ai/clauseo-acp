//! Copies clauseo-host's tracked-changes scan (crates/host/src/revisions/marks.rs) into OUT_DIR
//! for the zip_marks target, which `include!`s it, so the target runs the scan's own code without
//! depending on clauseo-host (its dependency tree would more than double this lock).
//!
//! Three edits make the file includable: its `//!` header goes (an included file may not hold
//! inner attributes), each `mod name;` gets the absolute path of the file it names (an included
//! module would look for it beside this crate), and a `fuzz_zip_has_marks` is appended inside the
//! module, so the private `zip_has_marks` can be called from outside it.
//!
//! Before that it checks the host paths the scan reaches (`crate::` and `super::`, comments and
//! test modules aside) against the ones zip_marks.rs stands in for, and fails the build naming
//! marks.rs when there is another, so a nightly break points at its cause.

use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let revisions = Path::new(&manifest).join("../crates/host/src/revisions");
    let source = revisions.join("marks.rs");
    let folder = revisions.join("marks");
    println!("cargo::rerun-if-changed={}", source.display());
    println!("cargo::rerun-if-changed={}", folder.display());
    let text = std::fs::read_to_string(&source).expect("crates/host/src/revisions/marks.rs");
    check_host_paths(
        "marks.rs",
        &text,
        &["super::is_docx", "crate::write::open_read_nofollow"],
    );
    for file in std::fs::read_dir(&folder)
        .expect("crates/host/src/revisions/marks/")
        .flatten()
    {
        let name = file.file_name().to_string_lossy().into_owned();
        if name.ends_with(".rs") && !name.contains("tests") && name != "reparse.rs" {
            let part = std::fs::read_to_string(file.path()).expect("a marks/ module");
            // `super::` in marks/x.rs is marks.rs itself, compiled here
            check_host_paths(&format!("marks/{name}"), &part, &["super::"]);
        }
    }
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//!") {
            continue;
        }
        if let Some(name) = trimmed
            .strip_prefix("mod ")
            .and_then(|rest| rest.strip_suffix(';'))
        {
            let file = folder.join(format!("{name}.rs"));
            out.push_str(&format!("#[path = {:?}]\n", file.display().to_string()));
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(
        "\n/// The zip_marks target's way in: [`zip_has_marks`] on `bytes`, held in memory.\n\
         pub(crate) fn fuzz_zip_has_marks(bytes: &[u8]) -> Option<bool> {\n    \
         zip_has_marks(std::io::Cursor::new(bytes))\n}\n",
    );
    let dest = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("marks.rs");
    #[expect(
        clippy::disallowed_methods,
        reason = "a build script writes its own OUT_DIR file"
    )]
    std::fs::write(dest, out).expect("write OUT_DIR/marks.rs");
}

/// Fails the build when `text` (`file` under crates/host/src/revisions/, its `cfg(test)` and
/// `cfg(windows)` code aside) reaches the host through a path that starts with none of `allowed`.
fn check_host_paths(file: &str, text: &str, allowed: &[&str]) {
    // a top-level cfg(test) or cfg(windows) item is skipped to its end (a `}` or a `;` line at
    // the top level); an indented one, its next line
    let mut skip_item = false;
    let mut skip_line = false;
    for (number, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if skip_item {
            skip_item = !(line.starts_with('}') || !line.starts_with(' ') && line.ends_with(';'));
            continue;
        }
        if std::mem::take(&mut skip_line) || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[cfg(windows)]") {
            if line.starts_with('#') {
                skip_item = true;
            } else {
                skip_line = true;
            }
            continue;
        }
        for start in ["crate::", "super::"] {
            for (at, _) in line.match_indices(start) {
                let path = &line[at..];
                if !allowed.iter().any(|ok| path.starts_with(ok)) {
                    panic!(
                        "zip_marks: crates/host/src/revisions/{file}:{} reaches the host through \
                         `{}`, which fuzz/fuzz_targets/zip_marks.rs has no stand-in for: add one \
                         there and to the allowed list in fuzz/build.rs",
                        number + 1,
                        path.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                            .next()
                            .unwrap_or(path),
                    );
                }
            }
        }
    }
}
