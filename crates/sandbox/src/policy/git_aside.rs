//! The macOS git watch's places in the policy (decision record 0038): the set-aside folders
//! (the data dir's, and a `.clauseo` folder at the root of the matter's volume when that is
//! another volume), the targets of the matter's launch `commondir` files, and whether the
//! watch's stream can watch the matter's volume (`git_aside/volume.rs`).

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::build::{beneath_existing, refuse};
use super::{PathRule, Policy, under};
use crate::Error;
use crate::policy::opens_as;

mod volume;

pub use volume::MatterWatch;
#[cfg(test)]
pub(crate) use volume::{Volume, fake_volume};

#[cfg(target_os = "macos")]
/// The folder at the root of another volume that the git watch sets aside into there.
const VOLUME_FOLDER: &str = ".clauseo";

/// The names git reads from a common dir: a `commondir` naming a folder the agent can write
/// would let it build them there.
const COMMON_NAMES: [&str; 7] = [
    "objects",
    "refs",
    "config",
    "packed-refs",
    "hooks",
    "info",
    "worktrees",
];

/// The most bytes of a `commondir` file read.
const COMMONDIR_MAX: u64 = 4096;

/// A launch repository's `commondir` target outside every read-only rule: git would read
/// objects, refs and config from it, so the agent must not make them there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommonDir {
    /// Nothing is there: the profile denies making it, or anything below it.
    Missing(PathBuf),
    /// An existing folder (the matter, the state folder, any other): the profile denies the
    /// names git reads in it, and only those.
    Folder(PathBuf),
}

impl CommonDir {
    /// The paths the profile denies writes at and below, and the watch sets aside when new.
    pub(crate) fn denied(&self) -> Vec<PathBuf> {
        match self {
            CommonDir::Missing(path) => vec![path.clone()],
            CommonDir::Folder(dir) => COMMON_NAMES.iter().map(|name| dir.join(name)).collect(),
        }
    }
}

/// The git watch's places in a policy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GitPlaces {
    /// The data dir's git set-aside folder ([`Policy::git_set_aside`]), canonical.
    pub(crate) set_aside: Option<PathBuf>,
    /// The matter volume's `.clauseo` folder ([`Policy::volume_set_aside`]): a rename can't
    /// cross volumes. The profile denies it whole.
    pub(crate) volume: Option<PathBuf>,
    /// The launch repositories' `commondir` targets outside every read-only rule.
    pub(crate) commondir: Vec<CommonDir>,
    /// Whether the watch's stream can watch the matter's volume.
    pub(crate) matter_watch: MatterWatch,
}

/// The places from the builder's set-aside folder, canonical (it need not exist); the rest is
/// [`finish`]'s.
pub(super) fn places(set_aside: Option<&Path>) -> Result<GitPlaces, Error> {
    Ok(GitPlaces {
        set_aside: set_aside
            .map(|dir| beneath_existing(dir, "git set-aside folder"))
            .transpose()?,
        ..GitPlaces::default()
    })
}

/// The policy with its git places checked and completed: the set-aside folder's rules, the
/// `commondir` targets and the volume set-aside folder.
pub(super) fn finish(policy: Policy) -> Result<Policy, Error> {
    let mut policy = check_git_set_aside(policy)?;
    policy.git.commondir =
        commondir_targets(&policy.read_only_in_matter, &policy.read_only_in_matter);
    #[cfg(target_os = "macos")]
    {
        let reach = reach(&policy);
        policy.git.volume = volume_set_aside(policy.matter(), policy.git_set_aside(), &reach)?;
    }
    policy.git.matter_watch = volume::matter_watch(&policy)?;
    Ok(policy)
}

/// The `commondir` targets of `repos` (the matter's `.git`s, `gitdir:` folders and bare
/// repositories, all read-only) that no read-only rule covers.
fn commondir_targets(repos: &[PathRule], read_only: &[PathRule]) -> Vec<CommonDir> {
    let covered = |path: &Path| {
        read_only
            .iter()
            .any(|rule| matches!(rule, PathRule::Subpath(dir) if under(path, dir)))
    };
    let mut out: Vec<CommonDir> = Vec::new();
    for repo in repos {
        let Some(target) = commondir_of(repo.path()) else {
            continue;
        };
        let found = match fs::metadata(&target) {
            Ok(meta) if meta.is_dir() => dunce::canonicalize(&target).ok().map(CommonDir::Folder),
            Ok(_) => dunce::canonicalize(&target).ok().map(CommonDir::Missing),
            Err(_) => beneath_existing(&target, "commondir target")
                .ok()
                .map(CommonDir::Missing),
        };
        let Some(found) = found else { continue };
        let path = match &found {
            CommonDir::Missing(path) | CommonDir::Folder(path) => path,
        };
        if !covered(path) && !out.contains(&found) {
            tracing::warn!(
                repo = %repo.path().display(), target = %path.display(),
                "a launch repository's commondir leads outside its read-only rules; denied there"
            );
            out.push(found);
        }
    }
    out
}

/// Where the `commondir` file in folder `dir` leads, made absolute and with `.` and `..`
/// taken lexically; `None` without one.
fn commondir_of(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let file = entries
        .flatten()
        .find(|entry| opens_as(&entry.file_name(), "commondir"))?
        .path();
    let meta = fs::symlink_metadata(&file).ok()?;
    if !meta.is_file() || meta.len() > COMMONDIR_MAX {
        return None;
    }
    let text = fs::read_to_string(&file).ok()?;
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let mut out = PathBuf::new();
    for part in dir.join(line).components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other @ (Component::Prefix(_) | Component::RootDir | Component::Normal(_)) => {
                out.push(other);
            }
        }
    }
    Some(out)
}

#[cfg(target_os = "macos")]
/// The volume set-aside folder for `matter` when its volume is another than `set_aside`'s,
/// local, and the folder can serve: inside no path the agent reaches but the matter, and in
/// the matter only as the volume's own root. Refused when the volumes can't be read.
fn volume_set_aside(
    matter: &Path,
    set_aside: Option<&Path>,
    reach: &[&Path],
) -> Result<Option<PathBuf>, Error> {
    let Some(set_aside) = set_aside else {
        return Ok(None);
    };
    let Some(root) = other_volume_root(matter, set_aside)? else {
        return Ok(None);
    };
    let dir = root.join(VOLUME_FOLDER);
    let same = super::same;
    let meets = |p: &Path| !same(p, matter) && (under(&dir, p) || under(p, &dir));
    if let Some(p) = reach.iter().find(|p| meets(p)) {
        return refuse(format!(
            "the matter is on another volume, whose git set-aside folder {} would hold or sit \
             inside a path the agent reaches: {}",
            dir.display(),
            p.display()
        ));
    }
    if under(matter, &dir) || (under(&root, matter) && !same(&root, matter)) {
        return refuse(format!(
            "the matter's volume can't hold its git set-aside folder {} apart from the matter",
            dir.display()
        ));
    }
    Ok(Some(dir))
}

/// The root of `matter`'s volume when it is a local volume other than `set_aside`'s.
#[cfg(target_os = "macos")]
fn other_volume_root(matter: &Path, set_aside: &Path) -> Result<Option<PathBuf>, Error> {
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::MetadataExt as _;
    /// `statfs`'s flag for a volume on this Mac (`<sys/mount.h>`).
    const MNT_LOCAL: u32 = 0x0000_1000;
    let unreadable =
        |p: &Path, e: &dyn std::fmt::Display| format!("{} can't be read: {e}", p.display());
    let existing = set_aside
        .ancestors()
        .find(|p| p.exists())
        .unwrap_or(set_aside);
    let dev = |p: &Path| fs::metadata(p).map(|meta| meta.dev());
    let (theirs, mine) = match (dev(matter), dev(existing)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) => return refuse(unreadable(matter, &e)),
        (_, Err(e)) => return refuse(unreadable(existing, &e)),
    };
    if theirs == mine {
        return Ok(None);
    }
    let stat = match rustix::fs::statfs(matter) {
        Ok(stat) => stat,
        Err(e) => return refuse(unreadable(matter, &e)),
    };
    if stat.f_flags & MNT_LOCAL == 0 {
        // once #766 lands: use network_paths
        tracing::info!(
            matter = %matter.display(),
            "the matter is on a network volume, which the git watch never scans"
        );
        return Ok(None);
    }
    let name: Vec<u8> = stat
        .f_mntonname
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c.to_ne_bytes()[0])
        .collect();
    Ok(Some(PathBuf::from(std::ffi::OsStr::from_bytes(&name))))
}

/// The git set-aside folder's rules: inside a hidden path, and neither inside nor holding the
/// matter or a path allowed again, so the profile denies it whole.
fn check_git_set_aside(policy: Policy) -> Result<Policy, Error> {
    let Some(dir) = policy.git_set_aside() else {
        return Ok(policy);
    };
    let shown = dir.display();
    if !policy.hidden.iter().any(|h| under(dir, h)) {
        return refuse(format!(
            "the git set-aside folder must be inside a hidden path: {shown}"
        ));
    }
    if let Some(p) = reach(&policy)
        .iter()
        .find(|p| under(dir, p) || under(p, dir))
    {
        return refuse(format!(
            "the git set-aside folder may not hold or sit inside a path the agent reaches: \
             {shown} and {}",
            p.display()
        ));
    }
    Ok(policy)
}

/// Every path the agent reaches.
fn reach(policy: &Policy) -> Vec<&Path> {
    let mut reach = vec![policy.matter(), policy.state()];
    reach.extend(policy.session_tmpdir());
    reach.extend(policy.notes());
    reach.extend(policy.credential_link());
    reach.extend(policy.claude_store().map(|store| store.dir.as_path()));
    reach.extend(policy.browser_socket_dir());
    for paths in [&policy.runtime, &policy.read, &policy.allowed_folders] {
        reach.extend(paths.iter().map(PathBuf::as_path));
    }
    reach
}
