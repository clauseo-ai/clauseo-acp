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
    /// A path the profile was given, or found on disk, is one it must not use: a relative
    /// program path, a symlink inside a skills folder, a name that escapes it.
    #[error("{}: {reason}", path.display())]
    Path {
        /// The path.
        path: PathBuf,
        /// Why it is refused.
        reason: &'static str,
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

    pub(crate) fn path(path: &Path, reason: &'static str) -> Self {
        Self::Path {
            path: path.to_owned(),
            reason,
        }
    }

    pub(crate) fn skill(skill: &str, reason: impl Into<String>) -> Self {
        Self::Skill {
            skill: skill.to_owned(),
            reason: reason.into(),
        }
    }

    /// The error as a log line may show it (decision record 0047): its kind, the step and the
    /// OS error kind, never a path, a skill's name or stetdoc's output.
    pub fn log_text(&self) -> String {
        match self {
            Self::Io { action, source, .. } => format!("io: {action}: {:?}", source.kind()),
            Self::Stetdoc { .. } => "stetdoc".to_owned(),
            Self::Path { reason, .. } => format!("path: {reason}"),
            Self::Skill { .. } => "skill".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProfileError;
    use std::path::Path;

    #[test]
    fn log_text_names_the_kind_never_the_path_or_the_skill() {
        let path = Path::new("/Users/ada/Matters/Acme/skills/acme-memo/SKILL.md");
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        let cases = [
            (
                ProfileError::io("read", path, missing),
                "io: read: NotFound",
            ),
            (ProfileError::path(path, "a link"), "path: a link"),
            (ProfileError::skill("acme-memo", "no name line"), "skill"),
        ];
        for (error, shown) in cases {
            assert_eq!(error.log_text(), shown);
            assert!(!error.log_text().contains("acme"), "{}", error.log_text());
        }
    }
}
