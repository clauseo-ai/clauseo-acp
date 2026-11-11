//! Tests of the MCP import: a fake filesystem with fixture files for each source, on both OSes.
//! Secret-looking values in these fixtures are fake and named as such.

use super::path::{file_name, is_absolute, join};
use super::*;
use std::collections::BTreeMap;

/// A fake filesystem: files with text, and folders with their entries.
#[derive(Default)]
struct FakeFs {
    files: BTreeMap<String, String>,
    dirs: BTreeMap<String, Vec<String>>,
}

impl FakeFs {
    fn file(mut self, path: &str, text: &str) -> Self {
        self.files.insert(path.to_owned(), text.to_owned());
        self
    }

    /// An empty file, for a program that exists.
    fn program(self, path: &str) -> Self {
        self.file(path, "")
    }

    fn dir(mut self, path: &str, entries: &[&str]) -> Self {
        self.dirs.insert(
            path.to_owned(),
            entries.iter().map(|entry| (*entry).to_owned()).collect(),
        );
        self
    }
}

impl ImportFs for FakeFs {
    fn read_to_string(&self, p: &Path) -> Option<String> {
        self.files.get(&p.to_string_lossy().into_owned()).cloned()
    }

    fn is_file(&self, p: &Path) -> bool {
        self.files.contains_key(&p.to_string_lossy().into_owned())
    }

    fn list_dir(&self, p: &Path) -> Vec<PathBuf> {
        self.dirs
            .get(&p.to_string_lossy().into_owned())
            .map(|entries| entries.iter().map(PathBuf::from).collect())
            .unwrap_or_default()
    }
}

/// Environment variables from pairs.
fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    move |name: &str| map.get(name).cloned()
}

/// Runs the import with one OS, home, environment and filesystem.
fn run(
    os: Os,
    home: &str,
    pairs: &[(&str, &str)],
    fs: &FakeFs,
    existing: &[&str],
) -> Vec<ImportCandidate> {
    let lookup = vars(pairs);
    let env = Env {
        os,
        home: Path::new(home),
        vars: &lookup,
        fs,
    };
    let existing: Vec<String> = existing.iter().map(|name| (*name).to_owned()).collect();
    find_candidates(&env, &existing)
}

fn by_name<'a>(found: &'a [ImportCandidate], name: &str) -> &'a ImportCandidate {
    found
        .iter()
        .find(|candidate| candidate.name == name)
        .unwrap_or_else(|| panic!("no candidate named {name}: {found:?}"))
}

fn names(found: &[ImportCandidate]) -> Vec<&str> {
    found
        .iter()
        .map(|candidate| candidate.name.as_str())
        .collect()
}

fn local(command: &str, args: &[&str], env: &[(&str, &str)]) -> CandidateKind {
    CandidateKind::Local {
        command: PathBuf::from(command),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        env: env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

fn remote(url: &str, headers: &[(&str, &str)]) -> CandidateKind {
    CandidateKind::Remote {
        url: url.to_owned(),
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

const DESKTOP_MAC: &str =
    "/Users/ana/Library/Application Support/Claude/claude_desktop_config.json";
const DESKTOP_WIN: &str = "C:\\Users\\ana\\AppData\\Roaming\\Claude\\claude_desktop_config.json";

mod misc;
mod names;
mod resolve;
mod sources;
