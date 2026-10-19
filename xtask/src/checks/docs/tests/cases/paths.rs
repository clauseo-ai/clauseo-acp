//! The planted inputs of the docs check that hold backticked paths: the first word read as the
//! path, a line location, and the ` (planned)` marker, read in `ARCHITECTURE.md` only.

use super::{ARCHITECTURE, docs, guide};
use crate::checks::Inspection;

pub(crate) fn path_with_a_line_passes() -> Inspection {
    guide("path-line", "See `xtask/src/main.rs:1`.\n")
}

/// The first word of a span is the path, whatever follows it.
pub(crate) fn first_word_is_the_path() -> Inspection {
    docs(
        "first-word",
        vec![
            (
                "GUIDE.md",
                "See `crates/nope/lib.rs and more`, `crates/nope.rs --flag` and \
                 `crates/host/src/main.rs --help`.\n"
                    .to_owned(),
            ),
            ("crates/host/src/main.rs", "fn main() {}\n".to_owned()),
        ],
    )
}

/// A first word that is a pattern is not a path, whatever follows it.
pub(crate) fn first_word_pattern_passes() -> Inspection {
    guide("first-word-pattern", "See `crates/<name>/src and more`.\n")
}

/// Rohan's three marked spans, outside `ARCHITECTURE.md`: the marker is not read.
pub(crate) fn planned_marker_outside_architecture_fails() -> Inspection {
    docs(
        "planned-outside",
        vec![
            (
                "GUIDE.md",
                "Next: `crates/nope/lib.rs` (planned), `ui/componnets` (planned) and \
                 `docs/REAL.md` (planned).\n"
                    .to_owned(),
            ),
            ("docs/real.md", "# Real\n".to_owned()),
        ],
    )
}

/// `ARCHITECTURE.md` with `text` after its Crates table, and `xtask/src/main.rs` and
/// `docs/real.md` listed.
fn architecture(case: &str, text: &str) -> Inspection {
    docs(
        case,
        vec![
            ("ARCHITECTURE.md", format!("{ARCHITECTURE}\n{text}")),
            ("xtask/src/main.rs", "fn main() {}\n".to_owned()),
            ("docs/real.md", "# Real\n".to_owned()),
        ],
    )
}

pub(crate) fn planned_missing_path_passes() -> Inspection {
    architecture(
        "planned-missing",
        "Packages: `ui/components` (planned) and `ui/bindings` (planned).\n",
    )
}

pub(crate) fn planned_existing_path_fails() -> Inspection {
    architecture(
        "planned-existing",
        "The entry: `xtask/src/main.rs` (planned).\n",
    )
}

pub(crate) fn planned_case_variant_fails() -> Inspection {
    architecture("planned-case", "Next: `docs/REAL.md` (planned).\n")
}
