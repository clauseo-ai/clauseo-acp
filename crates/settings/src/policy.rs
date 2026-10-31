//! The policy layer: settings a firm's IT pushes, read once at start-up.
//!
//! Each [`PolicySource`] reads one store: the registry on Windows, managed preferences on macOS,
//! and a `policy.json` on every OS. Each store's raw input goes through a plain parser, compiled
//! and tested on every OS: [`registry::parse`], [`managed_prefs::parse`] and [`file::parse`].
//! Only the code that collects the raw input is per OS.
//!
//! Each parser returns what its store can report. The registry parser runs only once the shim
//! found the key, so it returns [`PolicyValues`]; managed preferences are asked key by key, so
//! their parser is the one that sees that nothing is forced and returns [`PolicyRead`]; a
//! `policy.json` that exists can still fail to parse, so its parser returns a `Result`. A new
//! store picks the shape that says what it can report.

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

/// The values a policy source set, typed, with a warning for each it left out. A known key whose
/// value did not type is kept too, to lock the key at its default.
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
    /// The source's values, as [`leaf::typed`] returned them. Its leaves must come from there:
    /// each key once, with its last value, since the lock loop in [`crate::Effective::load`]
    /// keeps the first leaf it sees for a key.
    pub(crate) fn new(leaves: Vec<Leaf>, warnings: Vec<Warning>) -> PolicyValues {
        debug_assert!(
            {
                let mut keys = std::collections::BTreeSet::new();
                leaves.iter().all(|leaf| keys.insert(leaf.key()))
            },
            "a policy source's leaves must come from leaf::typed: one leaf per key"
        );
        PolicyValues { leaves, warnings }
    }

    /// The dotted keys this source locks, each once, in the order of its last value in the
    /// source. A key whose value did not type sits where that value sits (locked at its
    /// default); a key a registry refused, for its value's type, comes after the rest.
    pub fn keys(&self) -> impl Iterator<Item = String> {
        self.leaves.iter().map(Leaf::key)
    }

    /// Whether the source set at least one valid value. Only such a source counts as applied.
    pub fn sets_a_value(&self) -> bool {
        self.leaves.iter().any(Leaf::is_value)
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
    /// Whether no source set a valid policy value. A source that could not be read, or whose
    /// every value was refused, does not count, though the keys it named are still locked.
    pub fn is_absent(&self) -> bool {
        self.applied.is_empty()
    }

    /// The sources that set at least one valid value, strongest first.
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
