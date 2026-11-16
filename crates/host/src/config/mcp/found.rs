//! The finder of Settings › Tools' import (`HostConfig::with_mcp_import`): what it found, the
//! files it looked at with their stamps, and this computer's finder, which records every file
//! the settings crate's finder reads or checks so a later scan can skip when none changed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use clauseo_settings::mcp_import::{
    CandidateKind, Env, ImportCandidate, ImportFs, RealEnv, find_candidates_counted,
};

/// When a file was last changed, and its size; `None` when it is not there or can't be read.
pub type McpFileStamp = Option<(SystemTime, u64)>;

/// What one run of the finder found.
#[derive(Debug, Default)]
pub struct McpFound {
    /// The servers, as the import keeps them.
    pub candidates: Vec<ImportCandidate>,
    /// How many servers (or Claude Code projects) the per-source read cap left unread.
    pub left_out: usize,
    /// Each file the run read or checked, with its stamp then; empty when the finder can't tell.
    pub files: Vec<(PathBuf, McpFileStamp)>,
}

/// Finds the MCP servers Claude Desktop, Claude Code and Codex set up. Both calls run off the
/// actor, on a bounded worker (`actor/mcp/finder.rs`).
pub trait McpImportFinder: Send + Sync {
    /// The servers found, given the names the lawyer's list already has.
    fn find(&self, existing: &[String]) -> McpFound;
    /// Each of `paths`' stamp now, in order: a scan whose files all match the last one's (and
    /// whose list is the same) is skipped.
    fn stamps(&self, paths: &[PathBuf]) -> Vec<McpFileStamp>;
}

/// The servers `find_candidates` finds under `env`, each local one's program as the settings
/// keep it: the bare file name (`npx`) when searching for that name under `env` finds the same
/// program and it lives under a version manager's or Homebrew's folder, both compared by
/// `canonical` (`bare.rs`), so a launch finds it again after an upgrade moves it; else the
/// absolute path found. `files` is left empty: the caller's `env.fs` knows what it read.
pub fn find_mcp_imports_in(
    env: &Env<'_>,
    canonical: &dyn Fn(&Path) -> Option<PathBuf>,
    existing: &[String],
) -> McpFound {
    let (mut candidates, left_out) = find_candidates_counted(env, existing);
    for candidate in &mut candidates {
        if let CandidateKind::Local { command, .. } = &mut candidate.kind
            && let Some(name) = super::bare::bare_name(env, canonical, command)
        {
            *command = PathBuf::from(name);
        }
    }
    McpFound {
        candidates,
        left_out,
        files: Vec::new(),
    }
}

/// This computer's finder: the process's environment, `dunce::canonicalize`, and the real
/// filesystem, each path it touches recorded with its stamp.
pub struct McpHereFinder;

impl McpImportFinder for McpHereFinder {
    fn find(&self, existing: &[String]) -> McpFound {
        let Some(real) = RealEnv::from_process() else {
            return McpFound::default();
        };
        let base = real.env();
        let recording = Recording {
            fs: base.fs,
            seen: Mutex::new(BTreeSet::new()),
        };
        let env = Env {
            fs: &recording,
            ..base
        };
        let canonical = |path: &Path| dunce::canonicalize(path).ok();
        let mut found = find_mcp_imports_in(&env, &canonical, existing);
        let seen = recording
            .seen
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        found.files = seen
            .into_iter()
            .map(|path| {
                let stamp = stamp(&path);
                (path, stamp)
            })
            .collect();
        found
    }

    fn stamps(&self, paths: &[PathBuf]) -> Vec<McpFileStamp> {
        paths.iter().map(|path| stamp(path)).collect()
    }
}

/// `path`'s change time and size now.
fn stamp(path: &Path) -> McpFileStamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// An [`ImportFs`] that remembers each path it was asked about.
struct Recording<'a> {
    fs: &'a dyn ImportFs,
    seen: Mutex<BTreeSet<PathBuf>>,
}

impl Recording<'_> {
    fn saw(&self, path: &Path) {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(path.to_owned());
    }
}

impl ImportFs for Recording<'_> {
    fn read_to_string(&self, p: &Path, max: u64) -> Option<String> {
        self.saw(p);
        self.fs.read_to_string(p, max)
    }

    fn is_file(&self, p: &Path) -> bool {
        self.saw(p);
        self.fs.is_file(p)
    }

    fn list_dir(&self, p: &Path) -> Vec<PathBuf> {
        self.saw(p);
        self.fs.list_dir(p)
    }
}
