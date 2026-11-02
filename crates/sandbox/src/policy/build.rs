//! [`PolicyBuilder::build`]: canonicalising the inputs and the refusals.

use std::io;
use std::path::{Component, Path, PathBuf};

use super::{ClaudeStore, Loopback, PROTECTED_NAMES, PathRule, Policy, PolicyBuilder};
use crate::Error;
use crate::links::scan_matter;

/// The checks and the canonical policy.
pub(super) fn build(b: PolicyBuilder) -> Result<Policy, Error> {
    let matter = existing(&b.matter, "matter")?;
    let state = existing(&b.state, "state folder")?;
    let home = b
        .home
        .as_deref()
        .map(|home| beneath_existing(home, "home folder"))
        .transpose()?;
    let hidden = b
        .hidden
        .iter()
        .map(|p| beneath_existing(p, "hidden path"))
        .collect::<Result<Vec<_>, _>>()?;
    check_matter(&matter, &state, home.as_deref(), &hidden)?;

    let notes = b
        .notes
        .as_deref()
        .map(|p| existing(p, "notes folder"))
        .transpose()?;
    not_inside(notes.as_deref(), &matter, "the notes folder")?;
    let credential_link = b
        .credential_link
        .as_deref()
        .map(|p| folder_and_name(p, "credential link"))
        .transpose()?;
    not_inside(credential_link.as_deref(), &matter, "the credential link")?;
    let claude_store = b
        .claude_store
        .as_deref()
        .map(|dir| existing(dir, "Claude store folder").map(|dir| ClaudeStore { dir }))
        .transpose()?;

    let runtime = all_existing(&b.runtime, "runtime path")?;
    let read = all_existing(&b.read, "read path")?;
    let read_only_in_state = all_inside(
        &b.read_only_in_state,
        &state,
        "read-only path in the state folder",
    )?;
    let over_cap = all_inside(&b.over_cap, &matter, "over-cap file")?;
    let mut allowed_folders = Vec::new();
    for p in &b.allowed_folders {
        let folder = existing(p, "allowed folder")?;
        check_allowed(&folder, &matter, home.as_deref(), &hidden)?;
        allowed_folders.push(folder);
    }

    let mut scan = match b.scan {
        Some(scan) => scan,
        None => scan_matter(&matter)?,
    };
    scan.add_links(b.links);
    let (links, git, git_dirs) = scan.into_parts();
    let mut read_only_in_matter: Vec<PathRule> = PROTECTED_NAMES
        .iter()
        .map(|name| PathRule::Subpath(matter.join(name)))
        .chain(git.into_iter().map(PathRule::Subpath))
        .chain(git_dirs.into_iter().map(PathRule::Subpath))
        .chain(over_cap.iter().cloned().map(PathRule::Literal))
        .collect();
    read_only_in_matter.sort();
    read_only_in_matter.dedup();

    Ok(Policy {
        matter,
        state,
        home,
        runtime,
        read,
        allowed_folders,
        read_only_in_state,
        notes,
        credential_link,
        claude_store,
        hidden,
        proxy_ports: sorted(b.proxy_ports),
        loopback: match b.loopback {
            Loopback::Ports(ports) => Loopback::Ports(sorted(ports)),
            other @ (Loopback::None | Loopback::Any) => other,
        },
        keychain: b.keychain,
        over_cap,
        links,
        read_only_in_matter,
    })
}

/// The matter's own rules, on canonical paths.
fn check_matter(
    matter: &Path,
    state: &Path,
    home: Option<&Path>,
    hidden: &[PathBuf],
) -> Result<(), Error> {
    let shown = matter.display();
    if is_root(matter) {
        return refuse(format!("the matter may not be the root folder: {shown}"));
    }
    if home == Some(matter) {
        return refuse(format!("the matter may not be the home folder: {shown}"));
    }
    if let Some(h) = hidden.iter().find(|h| overlap(matter, h)) {
        return refuse(format!(
            "the matter may not equal, hold or sit inside a hidden path: {shown} and {}",
            h.display()
        ));
    }
    if overlap(state, matter) {
        return refuse(format!(
            "the state folder and the matter may not be one inside the other: {} and {shown}",
            state.display()
        ));
    }
    Ok(())
}

/// Refuses `p` (`what`, with its article) inside the matter.
fn not_inside(p: Option<&Path>, matter: &Path, what: &str) -> Result<(), Error> {
    match p {
        Some(p) if p.starts_with(matter) => refuse(format!(
            "{what} may not be inside the matter: {}",
            p.display()
        )),
        Some(_) | None => Ok(()),
    }
}

/// Each of `paths` by [`folder_and_name`], each of which must be inside `root`.
fn all_inside(paths: &[PathBuf], root: &Path, what: &str) -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let p = folder_and_name(p, what)?;
        if !p.starts_with(root) {
            return refuse(format!(
                "the {what} must be inside {}: {}",
                root.display(),
                p.display()
            ));
        }
        out.push(p);
    }
    Ok(out)
}

/// `v` sorted and de-duplicated.
fn sorted(mut v: Vec<u16>) -> Vec<u16> {
    v.sort_unstable();
    v.dedup();
    v
}

/// The allowed-folder rules, on a canonical `folder`.
pub(super) fn check_allowed(
    folder: &Path,
    matter: &Path,
    home: Option<&Path>,
    hidden: &[PathBuf],
) -> Result<(), Error> {
    let shown = folder.display();
    if folder
        .components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case(".git"))
    {
        return refuse(format!("an allowed folder may not be in a .git: {shown}"));
    }
    if is_root(folder) {
        return refuse(format!(
            "an allowed folder may not be the root folder: {shown}"
        ));
    }
    if home.is_some_and(|home| home.starts_with(folder)) {
        return refuse(format!(
            "an allowed folder may not equal or hold the home folder: {shown}"
        ));
    }
    if overlap(folder, matter) {
        return refuse(format!(
            "an allowed folder may not equal, hold or sit inside the matter: {shown} and {}",
            matter.display()
        ));
    }
    for h in hidden {
        if overlap(folder, h) {
            return refuse(format!(
                "an allowed folder may not equal, hold or sit inside a hidden path: {shown} and {}",
                h.display()
            ));
        }
    }
    Ok(())
}

/// `p`, which must be absolute and exist, canonical.
pub(super) fn existing(p: &Path, what: &str) -> Result<PathBuf, Error> {
    absolute(p, what)?;
    dunce::canonicalize(p).map_err(|error| cannot_resolve(p, what, &error))
}

/// Each of `paths`, by [`existing`].
fn all_existing(paths: &[PathBuf], what: &str) -> Result<Vec<PathBuf>, Error> {
    paths.iter().map(|p| existing(p, what)).collect()
}

/// `p`'s folder canonical, with `p`'s own name appended unresolved, so a link stays the link.
fn folder_and_name(p: &Path, what: &str) -> Result<PathBuf, Error> {
    absolute(p, what)?;
    let (Some(folder), Some(name)) = (p.parent(), p.file_name()) else {
        return refuse(format!("the {what} has no file name: {}", p.display()));
    };
    let folder = dunce::canonicalize(folder).map_err(|error| cannot_resolve(p, what, &error))?;
    Ok(folder.join(name))
}

/// `p` canonical as far as it exists: the longest existing ancestor canonicalised, the rest
/// appended. The rest may not hold `..`.
fn beneath_existing(p: &Path, what: &str) -> Result<PathBuf, Error> {
    absolute(p, what)?;
    let mut rest = Vec::new();
    let mut at = p;
    loop {
        if let Ok(found) = dunce::canonicalize(at) {
            if rest.contains(&Component::ParentDir) {
                return refuse(format!(
                    "the {what} has `..` past the part that exists: {}",
                    p.display()
                ));
            }
            return Ok(rest.iter().rev().fold(found, |path, c| path.join(c)));
        }
        let Some(parent) = at.parent() else {
            return refuse(format!("the {what} cannot be resolved: {}", p.display()));
        };
        if let Some(last) = at.components().next_back() {
            rest.push(last);
        }
        at = parent;
    }
}

/// Refuses a relative `p`.
fn absolute(p: &Path, what: &str) -> Result<(), Error> {
    if p.is_absolute() {
        Ok(())
    } else {
        refuse(format!(
            "the {what} must be an absolute path: {}",
            p.display()
        ))
    }
}

/// Whether `a` and `b` are the same or one holds the other.
fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// Whether `p` is a root (`/`, or a drive root on Windows).
fn is_root(p: &Path) -> bool {
    p.parent().is_none()
}

fn cannot_resolve(p: &Path, what: &str, error: &io::Error) -> Error {
    Error::Policy(format!(
        "the {what} must exist and be readable: {}: {error}",
        p.display()
    ))
}

fn refuse<T>(message: String) -> Result<T, Error> {
    Err(Error::Policy(message))
}
