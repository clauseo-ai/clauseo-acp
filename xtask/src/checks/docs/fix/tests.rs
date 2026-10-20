//! Pins the text of every `docs::Fix` variant. The `match` has no `_` arm, so a new variant does
//! not compile until it has a pinned text here.

use super::Fix;
use std::collections::HashSet;
use std::mem::discriminant;

fn pinned(fix: &Fix) -> &'static str {
    match fix {
        Fix::CreateImport { .. } => {
            "create CLAUDE.md holding only `@AGENTS.md`, or restore it from git history"
        }
        Fix::CreateRequired { .. } => {
            "create AGENTS.md at the repo root, or restore it from git history"
        }
        Fix::CutLines { .. } => {
            "cut AGENTS.md to 120 lines, moving what you cut into the file that owns it"
        }
        Fix::CutBytes { .. } => {
            "cut AGENTS.md to 8192 bytes, moving what you cut into the file that owns it"
        }
        Fix::MoveImport { .. } => {
            "move what CLAUDE.md holds besides `@AGENTS.md` into AGENTS.md, then keep only \
             `@AGENTS.md` in CLAUDE.md"
        }
        Fix::NoDotParts => "write the path from the repo root, without `..`",
        Fix::DropPlanned { .. } => "drop the (planned) after `crates/x/src/lib.rs`",
        Fix::NameTrackedFile => "name a file git tracks, or remove the reference",
        Fix::CreateOrCorrectReference { .. } => {
            "create `crates/nope/src/lib.rs`, or correct or remove the reference"
        }
        Fix::WriteCase { .. } => "write `docs/real.md`, and drop the (planned) after it",
        Fix::WriteUnderFolder { .. } => "write the path under `docs`, as git lists it",
        Fix::LinkTrackedFile => "link a file git tracks, or remove the link",
        Fix::CreateOrCorrectLink { .. } => "create `docs/nope.md`, or correct or remove the link",
        Fix::PointInside => "point it at a file inside the repo, or remove it",
        Fix::CorrectCommand => {
            "correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> \
             --help` lists its flags), or remove it"
        }
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::CreateImport {
            file: "CLAUDE.md".to_owned(),
            target: "AGENTS.md".to_owned(),
        },
        Fix::CreateRequired {
            file: "AGENTS.md".to_owned(),
        },
        Fix::CutLines {
            file: "AGENTS.md".to_owned(),
            limit: 120,
        },
        Fix::CutBytes {
            file: "AGENTS.md".to_owned(),
            limit: 8192,
        },
        Fix::MoveImport {
            file: "CLAUDE.md".to_owned(),
            target: "AGENTS.md".to_owned(),
        },
        Fix::NoDotParts,
        Fix::DropPlanned {
            path: "crates/x/src/lib.rs".to_owned(),
        },
        Fix::NameTrackedFile,
        Fix::CreateOrCorrectReference {
            path: "crates/nope/src/lib.rs".to_owned(),
        },
        Fix::WriteCase {
            real: "docs/real.md".to_owned(),
        },
        Fix::WriteUnderFolder {
            real: "docs".to_owned(),
        },
        Fix::LinkTrackedFile,
        Fix::CreateOrCorrectLink {
            path: "docs/nope.md".to_owned(),
        },
        Fix::PointInside,
        Fix::CorrectCommand,
    ];
    let kinds: HashSet<_> = samples.iter().map(discriminant).collect();
    assert_eq!(
        kinds.len(),
        samples.len(),
        "two samples are the same variant"
    );
    for fix in &samples {
        assert_eq!(fix.to_string(), pinned(fix));
    }
}
