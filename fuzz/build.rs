//! Copies clauseo-host's tracked-changes scan (crates/host/src/revisions/marks.rs) into OUT_DIR
//! for the zip_marks target, which `include!`s it, so the target runs the scan's own code without
//! depending on clauseo-host (its dependency tree would more than double this lock).
//!
//! Three edits make the file includable: its `//!` header goes (an included file may not hold
//! inner attributes), each `mod name;` gets the absolute path of the file it names (an included
//! module would look for it beside this crate), and a `fuzz_zip_has_marks` is appended inside the
//! module, so the private `zip_has_marks` can be called from outside it.

use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let revisions = Path::new(&manifest).join("../crates/host/src/revisions");
    let source = revisions.join("marks.rs");
    let folder = revisions.join("marks");
    println!("cargo::rerun-if-changed={}", source.display());
    println!("cargo::rerun-if-changed={}", folder.display());
    let text = std::fs::read_to_string(&source).expect("crates/host/src/revisions/marks.rs");
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
    std::fs::write(dest, out).expect("write OUT_DIR/marks.rs");
}
