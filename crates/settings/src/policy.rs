//! The policy layer: settings a firm's IT pushes, read once at start-up.
//!
//! Each [`PolicySource`] reads one store: the registry on Windows, managed preferences on macOS,
//! and a `policy.json` on every OS. Each store's raw input goes through a plain parser, compiled
//! and tested on every OS: [`registry::parse`], [`managed_prefs::parse`] and [`file::parse`].
//! Only the code that collects the raw input is per OS.

pub mod file;
pub mod managed_prefs;
pub mod registry;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use crate::error::Error;
use crate::files::Files;
use crate::leaf::{self, Leaf};
use crate::warning::Warning;
use std::path::PathBuf;

/// One place policy can come from.
pub trait PolicySource {
    /// The store's name, for warnings and [`PolicyStatus`].
    fn name(&self) -> &str;
    /// Reads the store. Called once, at start-up.
    fn read(&self) -> PolicyRead;
}

/// What reading one [`PolicySource`] found.
#[derive(Debug)]
pub enum PolicyRead {
    /// The store holds no policy.
    Absent,
    /// The store holds a policy.
    Read(PolicyValues),
    /// The store holds something that could not be read. Start-up goes on without this source,
    /// and [`PolicyStatus`] reports it.
    Unreadable(Error),
}

/// The values a policy source set, typed, with a warning for each it left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolicyValues {
    leaves: Vec<Leaf>,
    warnings: Vec<Warning>,
}

/// The policy layer after start-up.
#[derive(Debug, Default)]
pub struct PolicyStatus {
    applied: Vec<String>,
    errors: Vec<SourceError>,
}

/// A source that could not be read.
#[derive(Debug)]
pub struct SourceError {
    /// The source's name.
    pub source: String,
    /// What went wrong.
    pub error: Error,
}

/// How a store's raw value is read, by the kind of the setting it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// `true` or `false`.
    Boolean,
    /// A number.
    Number,
    /// A string.
    String,
    /// An object, such as an agent's launch spec.
    Object,
    /// A list of strings.
    StringArray,
}

/// The kind of the setting `key` names, or `None` for a key this version does not know.
pub fn kind_of(key: &str) -> Option<Kind> {
    match leaf::KEYS.iter().find(|(known, _)| *known == key) {
        Some(&(_, kind)) => Some(kind),
        None => leaf::agent_id(key).map(|_| Kind::Object),
    }
}

/// A `policy.json`, read through [`Files`].
#[derive(Debug, Clone)]
pub struct FilePolicy<F> {
    path: PathBuf,
    name: String,
    files: F,
}

/// The policy sources of this OS, strongest first: on Windows the registry, on macOS managed
/// preferences, then the OS's `policy.json`. No variable moves them.
pub fn system_policy_sources() -> Vec<Box<dyn PolicySource>> {
    #[cfg(windows)]
    return windows::sources();
    #[cfg(target_os = "macos")]
    return macos::sources();
    #[cfg(not(any(windows, target_os = "macos")))]
    return vec![Box::new(FilePolicy::new(
        PathBuf::from("/etc/clauseo/policy.json"),
        crate::SystemFiles,
    ))];
}

impl PolicyValues {
    pub(crate) fn new(leaves: Vec<Leaf>, warnings: Vec<Warning>) -> PolicyValues {
        PolicyValues { leaves, warnings }
    }

    /// The dotted keys this source sets, in its order.
    pub fn keys(&self) -> impl Iterator<Item = String> {
        self.leaves.iter().map(Leaf::key)
    }

    /// The values it left out, and why.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    pub(crate) fn into_parts(self) -> (Vec<Leaf>, Vec<Warning>) {
        (self.leaves, self.warnings)
    }
}

impl PolicyStatus {
    /// Whether no source holds a policy. A source that could not be read does not count.
    pub fn is_absent(&self) -> bool {
        self.applied.is_empty()
    }

    /// The sources that hold a policy, strongest first.
    pub fn applied(&self) -> &[String] {
        &self.applied
    }

    /// The sources that could not be read; start-up went on without them.
    pub fn errors(&self) -> &[SourceError] {
        &self.errors
    }

    pub(crate) fn push_applied(&mut self, source: &str) {
        self.applied.push(source.to_owned());
    }

    pub(crate) fn push_error(&mut self, source: &str, error: Error) {
        self.errors.push(SourceError {
            source: source.to_owned(),
            error,
        });
    }
}

impl<F: Files> FilePolicy<F> {
    /// The `policy.json` at `path`.
    pub fn new(path: PathBuf, files: F) -> FilePolicy<F> {
        FilePolicy {
            name: path.display().to_string(),
            path,
            files,
        }
    }
}

impl<F: Files> PolicySource for FilePolicy<F> {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> PolicyRead {
        match self.files.read(&self.path) {
            Ok(None) => PolicyRead::Absent,
            Ok(Some(bytes)) => match file::parse(&self.path, &bytes) {
                Ok(values) => PolicyRead::Read(values),
                Err(error) => PolicyRead::Unreadable(error),
            },
            Err(source) => PolicyRead::Unreadable(Error::Io {
                path: self.path.clone(),
                source,
            }),
        }
    }
}
