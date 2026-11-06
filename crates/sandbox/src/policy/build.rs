//! [`PolicyBuilder::build`]: canonicalising the inputs and the refusals.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::links::{canonical_findings, covered_targets, fit_budget, split_linked};
use super::names::{Keys, path_key};
use super::{
    ClaudeStore, Loopback, PROTECTED_AT_ANY_DEPTH, PROTECTED_NAMES, PathRule, Policy,
    PolicyBuilder, same, under,
};
use crate::Error;
use crate::links::{LinkFinding, scan_matter};

/// The checks and the canonical policy.
pub(super) fn build(b: PolicyBuilder) -> Result<Policy, Error> {
    let matter = root_folder(&b.matter, "matter")?;
    let state = root_folder(&b.state, "state folder")?;
    let home = b
        .home
        .as_deref()
        .map(|home| beneath_existing(home, "home folder"))
        .transpose()?;
    let hidden = all_beneath(&b.hidden, "hidden path")?;
    let home_ref = home.as_deref();
    check_matter(&matter, &state, home_ref, &hidden)?;
    let tmpdir = tmp_folder(b.tmpdir.as_deref(), &matter, &state, home_ref, &hidden)?;

    let notes = b
        .notes
        .as_deref()
        .map(|p| root_folder(p, "notes folder"))
        .transpose()?;
    if let Some(notes) = &notes {
        check_notes(notes, &matter, &hidden)?;
    }
    let credential_link = b
        .credential_link
        .as_deref()
        .map(|p| folder_and_name(p, "credential link"))
        .transpose()?;
    if let Some(link) = &credential_link
        && (!under(link, &state) || same(link, &state))
    {
        return refuse(format!(
            "the credential link must be inside the state folder {}: {}",
            state.display(),
            link.display()
        ));
    }
    let claude_store = b
        .claude_store
        .as_deref()
        .map(|dir| existing(dir, "Claude store folder").map(|dir| ClaudeStore { dir }))
        .transpose()?;

    let runtime = all_existing(&b.runtime, "runtime path")?;
    let read = all_existing(&b.read, "read path")?;
    for (paths, what) in [(&runtime, "a runtime path"), (&read, "a read path")] {
        for p in paths {
            check_reallowed(p, what, &matter, home_ref, &hidden)?;
        }
    }
    let read_only_in_state = all_inside(
        &b.read_only_in_state,
        &state,
        "read-only path in the state folder",
    )?;
    let over_cap = all_inside(&b.over_cap, &matter, "over-cap file")?;
    let mut allowed_folders = Vec::new();
    for p in &b.allowed_folders {
        let folder = existing(p, "allowed folder")?;
        check_allowed(&folder, &matter, home_ref, &hidden)?;
        allowed_folders.push(folder);
    }

    let mut scan = match b.scan {
        Some(scan) => scan,
        None => scan_matter(&matter)?,
    };
    scan.add_links(b.links);
    let parts = scan.into_parts();
    let working_trees = working_trees(&parts.git);
    let mut read_only_in_matter: Vec<PathRule> = PROTECTED_NAMES
        .iter()
        .map(|name| PathRule::Subpath(matter.join(name)))
        .chain(parts.git.into_iter().map(PathRule::Subpath))
        .chain(parts.git_dirs.into_iter().map(PathRule::Subpath))
        .chain(outermost(parts.nested, &matter))
        .chain(over_cap.iter().cloned().map(PathRule::Literal))
        .collect();
    read_only_in_matter.sort();
    // One rule per path and kind, however its names are written (over-cap files come from the
    // host).
    let mut seen = HashSet::new();
    read_only_in_matter
        .retain(|rule| seen.insert((matches!(rule, PathRule::Literal(_)), path_key(rule.path()))));
    let (mut links, found) = canonical_findings(parts.links, &matter)?;
    let linked = split_linked(parts.linked, &matter, &read_only_in_matter);
    add_new(&mut links, &found, linked);
    let mut allowed: Vec<&Path> = vec![state.as_path()];
    allowed.extend(tmpdir.as_deref());
    allowed.extend(runtime.iter().map(PathBuf::as_path));
    allowed.extend(read.iter().map(PathBuf::as_path));
    allowed.extend(allowed_folders.iter().map(PathBuf::as_path));
    allowed.extend(notes.as_deref());
    let allowed_exactly = |p: &Path| {
        credential_link.as_deref().is_some_and(|link| same(p, link))
            || claude_store.as_ref().is_some_and(|store| store.covers(p))
    };
    let link_targets = covered_targets(&links, &allowed, &allowed_exactly);
    let link_folders = fit_budget(&links, &link_targets, &matter, &working_trees)?;

    Ok(Policy {
        matter,
        state,
        tmpdir,
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
        browser_socket_dir: b.browser_socket_dir,
        keychain: b.keychain,
        over_cap,
        links,
        link_targets,
        link_folders,
        read_only_in_matter,
    })
}

/// `extra` added to `links`, each unless a finding already there names its path: `found` holds
/// their [`path_key`](super::names::path_key)s, so each check is one lookup.
fn add_new(links: &mut Vec<LinkFinding>, found: &HashSet<PathBuf>, extra: Vec<LinkFinding>) {
    let mut keys = Keys::default();
    links.extend(
        extra
            .into_iter()
            .filter(|finding| !found.contains(&keys.of(&finding.path))),
    );
}

/// A subpath rule for each of `nested`, the [`PROTECTED_AT_ANY_DEPTH`] names the walk found in
/// `matter` besides `.git`, except one in a folder whose own rule covers it ([`in_denied_folder`]).
fn outermost(nested: Vec<PathBuf>, matter: &Path) -> impl Iterator<Item = PathRule> {
    nested
        .into_iter()
        .filter(move |p| !in_denied_folder(p, matter))
        .map(PathRule::Subpath)
}

/// Whether `path`, a [`PROTECTED_AT_ANY_DEPTH`] name the walk found in `matter`, is inside a
/// folder whose own rule covers it already: a folder of one of those names, or of one of
/// [`PROTECTED_NAMES`] at the root (`a/.claude/CLAUDE.md`, `.codex/AGENTS.md`). Names only,
/// in any letter case, as the walk matched them. A path not below `matter` as written (a scan
/// the caller gave of another spelling) is kept: one rule too many costs nothing.
fn in_denied_folder(path: &Path, matter: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(matter) else {
        return false;
    };
    let mut folders: Vec<&std::ffi::OsStr> = rest.iter().collect();
    folders.pop();
    let is =
        |names: &[&str], name: &std::ffi::OsStr| names.iter().any(|n| name.eq_ignore_ascii_case(n));
    folders
        .first()
        .is_some_and(|first| is(&PROTECTED_NAMES, first))
        || folders.iter().any(|name| is(&PROTECTED_AT_ANY_DEPTH, name))
}

/// The folders that hold a `.git`, repositories' working trees: neither they nor a folder above
/// them is ever a link folder.
fn working_trees(git: &[PathBuf]) -> Vec<PathBuf> {
    git.iter()
        .filter_map(|git| git.parent().map(Path::to_owned))
        .collect()
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
    if home.is_some_and(|home| under(home, matter)) {
        return refuse(format!(
            "the matter may not equal or hold the home folder: {shown}"
        ));
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
    if let Some(h) = hidden.iter().find(|h| under(h, state)) {
        return refuse(format!(
            "the state folder may not equal or hold a hidden path: {} and {}",
            state.display(),
            h.display()
        ));
    }
    Ok(())
}

/// The notes folder's rules, on canonical paths: it is allowed again after the hidden paths,
/// so it may not be in the matter, hold it, or equal or hold a hidden path.
fn check_notes(notes: &Path, matter: &Path, hidden: &[PathBuf]) -> Result<(), Error> {
    let shown = notes.display();
    if overlap(notes, matter) {
        return refuse(format!(
            "the notes folder may not be inside the matter or hold it: {shown} and {}",
            matter.display()
        ));
    }
    if let Some(h) = hidden.iter().find(|h| under(h, notes)) {
        return refuse(format!(
            "the notes folder may not equal or hold a hidden path: {shown} and {}",
            h.display()
        ));
    }
    Ok(())
}

/// The session's temp folder if set, by [`folder_and_name`] after refusing a `p` whose own name
/// is a symbolic link (the launcher makes the folder if it is missing). It is allowed again
/// after the hidden paths and holds the agent's writes, so it may not be `/`, equal or hold the
/// home folder, the state folder or a hidden path, nor be inside the matter or hold it.
fn tmp_folder(
    p: Option<&Path>,
    matter: &Path,
    state: &Path,
    home: Option<&Path>,
    hidden: &[PathBuf],
) -> Result<Option<PathBuf>, Error> {
    let Some(p) = p else {
        return Ok(None);
    };
    let what = "temp folder";
    absolute(p, what)?;
    if fs::symlink_metadata(p).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return refuse(format!(
            "the {what} may not be a symbolic link: {}",
            p.display()
        ));
    }
    let tmpdir = folder_and_name(p, what)?;
    let shown = tmpdir.display();
    if is_root(&tmpdir) {
        return refuse(format!("the {what} may not be the root folder: {shown}"));
    }
    if home.is_some_and(|home| under(home, &tmpdir)) {
        return refuse(format!(
            "the {what} may not equal or hold the home folder: {shown}"
        ));
    }
    if under(state, &tmpdir) {
        return refuse(format!(
            "the {what} may not equal or hold the state folder: {shown} and {}",
            state.display()
        ));
    }
    if overlap(&tmpdir, matter) {
        return refuse(format!(
            "the {what} may not be inside the matter or hold it: {shown} and {}",
            matter.display()
        ));
    }
    if let Some(h) = hidden.iter().find(|h| under(h, &tmpdir)) {
        return refuse(format!(
            "the {what} may not equal or hold a hidden path: {shown} and {}",
            h.display()
        ));
    }
    Ok(Some(tmpdir))
}

/// The rules of a runtime or read path (`what`, with its article), on canonical paths. Both
/// are allowed again after the hidden paths and the matter's read-only rules, so one may not be
/// `/` or equal or hold the home folder, the matter or a hidden path. Inside a hidden path is
/// fine: that is why they are allowed again.
fn check_reallowed(
    p: &Path,
    what: &str,
    matter: &Path,
    home: Option<&Path>,
    hidden: &[PathBuf],
) -> Result<(), Error> {
    let shown = p.display();
    if is_root(p) {
        return refuse(format!("{what} may not be the root folder: {shown}"));
    }
    if home.is_some_and(|home| under(home, p)) {
        return refuse(format!(
            "{what} may not equal or hold the home folder: {shown}"
        ));
    }
    if under(matter, p) {
        return refuse(format!(
            "{what} may not equal or hold the matter: {shown} and {}",
            matter.display()
        ));
    }
    if let Some(h) = hidden.iter().find(|h| under(h, p)) {
        return refuse(format!(
            "{what} may not equal or hold a hidden path: {shown} and {}",
            h.display()
        ));
    }
    Ok(())
}

/// The matter, the state folder or the notes folder (`what`): by [`existing`], after refusing a
/// `p` whose own name is a symbolic link. An agent can replace its own folder's contents, so a
/// link there may be one it planted to point the next launch at another folder.
fn root_folder(p: &Path, what: &str) -> Result<PathBuf, Error> {
    absolute(p, what)?;
    if fs::symlink_metadata(p).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return refuse(format!(
            "the {what} may not be a symbolic link: {}",
            p.display()
        ));
    }
    existing(p, what)
}

/// Each of `paths` by [`folder_and_name`], each of which must be inside `root`.
fn all_inside(paths: &[PathBuf], root: &Path, what: &str) -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let p = folder_and_name(p, what)?;
        if !under(&p, root) {
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
    if home.is_some_and(|home| under(home, folder)) {
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
pub(super) fn folder_and_name(p: &Path, what: &str) -> Result<PathBuf, Error> {
    absolute(p, what)?;
    let (Some(folder), Some(name)) = (p.parent(), p.file_name()) else {
        return refuse(format!("the {what} has no file name: {}", p.display()));
    };
    let folder = dunce::canonicalize(folder).map_err(|error| cannot_resolve(p, what, &error))?;
    Ok(folder.join(name))
}

/// Each of `paths`, by [`beneath_existing`].
fn all_beneath(paths: &[PathBuf], what: &str) -> Result<Vec<PathBuf>, Error> {
    paths.iter().map(|p| beneath_existing(p, what)).collect()
}

/// `p` canonical as far as it exists: the longest existing ancestor canonicalised, the rest
/// appended. The rest may not hold `..`.
pub(super) fn beneath_existing(p: &Path, what: &str) -> Result<PathBuf, Error> {
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

/// Whether `a` and `b` are the same or one holds the other, by [`under`].
fn overlap(a: &Path, b: &Path) -> bool {
    under(a, b) || under(b, a)
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

pub(super) fn refuse<T>(message: String) -> Result<T, Error> {
    Err(Error::Policy(message))
}
