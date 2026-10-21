//! The fix lines of the docs check, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use super::markdown::PLANNED;
use std::fmt;

/// A fix line of the docs check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A required file that is missing, whose rule is that it holds only `@target`.
    CreateImport { file: String, target: String },
    /// A required file that is missing, whose rule is its limits.
    CreateRequired { file: String },
    /// A required file over its line limit.
    CutLines { file: String, limit: usize },
    /// A required file over its byte limit.
    CutBytes { file: String, limit: usize },
    /// A required file that holds more than its `@target` import.
    MoveImport { file: String, target: String },
    /// A repo path that climbs with `..`.
    NoDotParts,
    /// A path marked planned that exists: the marker is dropped.
    DropPlanned { path: String },
    /// A path that is on disk but not listed by git: name a tracked file instead.
    NameTrackedFile,
    /// A repo path that does not exist.
    CreateOrCorrectReference { path: String },
    /// A planned path listed in another case: write the real name, and drop the marker.
    WriteCase { real: String },
    /// A planned path whose folder is listed in another case.
    WriteUnderFolder { real: String },
    /// A relative link into a target that is on disk but not listed by git.
    LinkTrackedFile,
    /// A relative link whose target does not exist.
    CreateOrCorrectLink { path: String },
    /// A relative link that leaves the repo.
    PointInside,
    /// An `cargo xtask` command that does not parse.
    CorrectCommand,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::CreateImport { file, target } => write!(
                f,
                "create {file} holding only `@{target}`, or restore it from git history"
            ),
            Fix::CreateRequired { file } => {
                write!(
                    f,
                    "create {file} at the repo root, or restore it from git history"
                )
            }
            Fix::CutLines { file, limit } => write!(
                f,
                "cut {file} to {limit} lines, moving what you cut into the file that owns it"
            ),
            Fix::CutBytes { file, limit } => write!(
                f,
                "cut {file} to {limit} bytes, moving what you cut into the file that owns it"
            ),
            Fix::MoveImport { file, target } => write!(
                f,
                "move what {file} holds besides `@{target}` into {target}, then keep only \
                 `@{target}` in {file}"
            ),
            Fix::NoDotParts => f.write_str("write the path from the repo root, without `..`"),
            Fix::DropPlanned { path } => write!(f, "drop the{PLANNED} after `{path}`"),
            Fix::NameTrackedFile => f.write_str("name a file git tracks, or remove the reference"),
            Fix::CreateOrCorrectReference { path } => {
                write!(f, "create `{path}`, or correct or remove the reference")
            }
            Fix::WriteCase { real } => {
                write!(f, "write `{real}`, and drop the{PLANNED} after it")
            }
            Fix::WriteUnderFolder { real } => {
                write!(f, "write the path under `{real}`, as git lists it")
            }
            Fix::LinkTrackedFile => f.write_str("link a file git tracks, or remove the link"),
            Fix::CreateOrCorrectLink { path } => {
                write!(f, "create `{path}`, or correct or remove the link")
            }
            Fix::PointInside => f.write_str("point it at a file inside the repo, or remove it"),
            Fix::CorrectCommand => f.write_str(
                "correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> \
                 --help` lists its flags), or remove it",
            ),
        }
    }
}

#[cfg(test)]
mod tests;
