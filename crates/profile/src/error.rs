//! The profile's errors.

use std::io;
use std::path::{Path, PathBuf};

/// What can go wrong writing the skills.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProfileError {
    /// A file step failed.
    #[error("could not {action} {}: {source}", path.display())]
    Io {
        /// The step: `create`, `write`, `remove`, `read`.
        action: &'static str,
        /// The path.
        path: PathBuf,
        /// The cause.
        source: io::Error,
    },
    /// The stetdoc binary did not answer as expected.
    #[error("stetdoc {args}: {reason}")]
    Stetdoc {
        /// The arguments it ran with.
        args: String,
        /// What went wrong.
        reason: String,
    },
    /// A skill's text breaks the format or a rule.
    #[error("the skill {skill}: {reason}")]
    Skill {
        /// The skill's name.
        skill: String,
        /// What is wrong.
        reason: String,
    },
}

impl ProfileError {
    pub(crate) fn io(action: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_owned(),
            source,
        }
    }

    pub(crate) fn skill(skill: &str, reason: impl Into<String>) -> Self {
        Self::Skill {
            skill: skill.to_owned(),
            reason: reason.into(),
        }
    }
}
