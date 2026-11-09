//! The walk under the root, as [`super`] describes it: each subkey taken over before it is read,
//! judged by its owner and DACL as they were, then kept or deleted with its subtree; the keys
//! the walk kept checked once more when it is done. Pure, against [`Tree`].

use std::collections::VecDeque;

use super::security::{self, Security};
use super::{
    INHERITED_SDDL, INSTALLER_NAME, LOCKED_SDDL, Report, Tree, VendorFault, pinned, unless_gone,
};

/// What a key the walk makes again starts with, owned by Administrators (which the caller may
/// assign) until it is taken over.
const CREATED_SDDL: &str = "O:BAD:P(A;;KA;;;SY)(A;;KA;;;BA)";

/// One key on the walk: its handle (taken over), its paths, the SD it was left with, and its
/// subkeys not yet visited.
struct Frame<K> {
    key: K,
    path: String,
    relative: String,
    target: String,
    names: VecDeque<String>,
}

/// A key the walk kept, for the check at its end: its handle, its path and the SD it was left
/// with.
pub(super) struct Kept<K> {
    key: K,
    path: String,
    target: String,
}

impl<K> Kept<K> {
    /// Its handle.
    pub(super) fn key(&self) -> &K {
        &self.key
    }
}

/// What a key is judged against: its pinned SD when the SYSTEM side trusts it, and this
/// machine's account domain.
struct Judged<'a> {
    pinned: Option<&'a str>,
    local: &'a Result<[u32; 3], String>,
}

/// A key's paths: under `HKLM`, and under the root.
struct At {
    path: String,
    relative: String,
}

/// What became of a key the walk visited.
enum Visit<K> {
    /// Kept, with its frame.
    Kept(Frame<K>),
    /// Deleted by the walk.
    Deleted,
    /// Gone under the walk.
    Gone,
}

/// Walks every subkey under `root` (left with `target`) with an explicit stack, depth first, and
/// gives `root` back with every key it kept, the root first.
pub(super) fn clean<T: Tree>(
    tree: &mut T,
    root: T::Key,
    path: &str,
    target: &str,
    report: &mut Report,
) -> Result<Vec<Kept<T::Key>>, VendorFault> {
    let local = tree.local_domain();
    let names = unless_gone(tree.subkeys(&root), VendorFault::CleanUp)?.unwrap_or_default();
    let mut kept = Vec::new();
    let mut stack = vec![Frame {
        key: root,
        path: path.to_owned(),
        relative: String::new(),
        target: target.to_owned(),
        names: names.into(),
    }];
    loop {
        let Some(frame) = stack.last_mut() else {
            // The root's frame is first in and last out, so it is first in `kept` once reversed.
            kept.reverse();
            return Ok(kept);
        };
        let Some(name) = frame.names.pop_front() else {
            if let Some(done) = stack.pop() {
                kept.push(Kept {
                    key: done.key,
                    path: done.path,
                    target: done.target,
                });
            }
            continue;
        };
        let child = At {
            path: format!("{}\\{name}", frame.path),
            relative: if frame.relative.is_empty() {
                name.clone()
            } else {
                format!("{}\\{name}", frame.relative)
            },
        };
        let pinned = pinned(&child.relative);
        let judged = Judged {
            pinned: pinned.as_deref(),
            local: &local,
        };
        let visited = visit(tree, &frame.key, &name, &child, &judged, report)?;
        let deleted = match visited {
            Visit::Kept(frame) => {
                stack.push(frame);
                continue;
            }
            Visit::Deleted => true,
            Visit::Gone => false,
        };
        let Some(parent) = stack.last() else {
            continue;
        };
        if let Some(pinned) = pinned.as_deref() {
            if unless_gone(
                tree.create_child(&parent.key, &name, pinned),
                VendorFault::Secure,
            )?
            .is_some()
            {
                report.recreated.push(child.path);
            }
        } else if deleted && child.relative.eq_ignore_ascii_case(INSTALLER_NAME) {
            remake_installer(tree, &parent.key, &name, child.path, report)?;
        }
    }
}

/// Makes the installer's key `name` under `parent` again, taken over with [`INHERITED_SDDL`],
/// with its install folder from the uninstall key.
fn remake_installer<T: Tree>(
    tree: &mut T,
    parent: &T::Key,
    name: &str,
    path: String,
    report: &mut Report,
) -> Result<(), VendorFault> {
    let Some(key) = unless_gone(
        tree.create_child(parent, name, CREATED_SDDL),
        VendorFault::Secure,
    )?
    else {
        return Ok(());
    };
    if unless_gone(tree.retake(&key, INHERITED_SDDL), VendorFault::Secure)?.is_none() {
        return Ok(());
    }
    if let Some(folder) = unless_gone(tree.reset_install_dir(&key), VendorFault::CleanUp)? {
        report.install_dir_reset = Some((path, folder));
    }
    Ok(())
}

/// Visits subkey `name` of `parent` (`at` gives its paths).
fn visit<T: Tree>(
    tree: &mut T,
    parent: &T::Key,
    name: &str,
    at: &At,
    judged: &Judged<'_>,
    report: &mut Report,
) -> Result<Visit<T::Key>, VendorFault> {
    let pinned = judged.pinned;
    // What the child was before the take-over, read on the take-over's own handle.
    let Some((key, before)) = unless_gone(
        tree.take_over_child(parent, name, LOCKED_SDDL),
        VendorFault::Secure,
    )?
    else {
        return Ok(Visit::Gone);
    };
    let before = Security::parse(&before);
    let owner = owner_of(tree, &before.owner, judged.local);
    let Some(link) = unless_gone(tree.has_link_value(&key), VendorFault::CleanUp)? else {
        return Ok(Visit::Gone);
    };
    // The SYSTEM side's keys get no benefit of the doubt.
    let planted = match &owner {
        Owner::Planter => true,
        Owner::Unresolved(_) => pinned.is_some(),
        Owner::Admin | Owner::OtherAdmin | Owner::Service => false,
    };
    let others_wrote = before.others_may_write();
    // The installer's key is trusted only when an administrator owned it and no one else could
    // write it: its folder is what the uninstaller deletes.
    let installer = at.relative.eq_ignore_ascii_case(INSTALLER_NAME);
    let untrusted_installer =
        installer && (others_wrote || !matches!(owner, Owner::Admin | Owner::OtherAdmin));
    if link || planted || (pinned.is_some() && others_wrote) || untrusted_installer {
        drain(tree, key)?;
        let list = if link {
            &mut report.removed
        } else if planted {
            &mut report.planted
        } else if pinned.is_some() {
            &mut report.writable_pinned
        } else {
            &mut report.untrusted_installer
        };
        list.push(at.path.clone());
        return Ok(Visit::Deleted);
    }
    let target = pinned.unwrap_or(INHERITED_SDDL);
    if unless_gone(tree.set_sddl(&key, target), VendorFault::Secure)?.is_none() {
        return Ok(Visit::Gone);
    }
    if others_wrote {
        report.others_could_write.push(at.path.clone());
    }
    match owner {
        Owner::OtherAdmin => report.admin_owned.push(at.path.clone()),
        Owner::Unresolved(why) => report.unresolved.push((at.path.clone(), why)),
        Owner::Admin | Owner::Service | Owner::Planter => {}
    }
    if pinned.is_some() {
        if before != Security::parse(target) {
            report.resecured.push(at.path.clone());
        }
    } else {
        report.taken_over.push(at.path.clone());
    }
    let names = unless_gone(tree.subkeys(&key), VendorFault::CleanUp)?.unwrap_or_default();
    Ok(Visit::Kept(Frame {
        key,
        path: at.path.clone(),
        relative: at.relative.clone(),
        target: target.to_owned(),
        names: names.into(),
    }))
}

/// Checks every key in `kept` against the SD the walk left it with, as a handle held from before
/// its take-over could have changed it since. A changed key is taken over again once; one still
/// changed after that is a fault, and set-up stops.
pub(super) fn verify<T: Tree>(
    tree: &mut T,
    kept: &[Kept<T::Key>],
    report: &mut Report,
) -> Result<(), VendorFault> {
    for key in kept {
        let wanted = Security::parse(&key.target);
        let Some(now) = unless_gone(tree.sddl(&key.key), VendorFault::Open)? else {
            continue;
        };
        if Security::parse(&now) == wanted {
            continue;
        }
        if unless_gone(tree.retake(&key.key, &key.target), VendorFault::Secure)?.is_none() {
            continue;
        }
        let Some(again) = unless_gone(tree.sddl(&key.key), VendorFault::Open)? else {
            continue;
        };
        if Security::parse(&again) != wanted {
            return Err(VendorFault::Secure(format!(
                "{} was changed back after its take-over",
                key.path
            )));
        }
        report.retaken.push(key.path.clone());
    }
    Ok(())
}

/// Who `sid`, a key's owner before the take-over, is. SYSTEM and Administrators, then the OS's
/// service accounts and service SIDs, are trusted; a well-known SID ordinary users hold (Users,
/// Everyone) is a planter, since an owner may always rewrite the DACL. An alias is spelled out
/// first. Only a local account is judged by membership. Anything else (another built-in group, a
/// domain account, an alias that can't be spelled out) is unresolved, as is a local account when
/// the local domain can't be read.
fn owner_of<T: Tree>(tree: &mut T, sid: &str, local: &Result<[u32; 3], String>) -> Owner {
    let spelled;
    let sid = if sid.starts_with("S-") {
        sid
    } else {
        match tree.spell_out(sid) {
            Ok(found) => {
                spelled = found.to_ascii_uppercase();
                spelled.as_str()
            }
            Err(why) => return Owner::Unresolved(format!("the alias {sid}: {why}")),
        }
    };
    if security::TRUSTED_OWNERS.contains(&sid) {
        return Owner::Admin;
    }
    if security::OS_OWNERS.contains(&sid) || sid.starts_with("S-1-5-80-") {
        return Owner::Service;
    }
    if security::USER_HELD_OWNERS.contains(&sid) {
        return Owner::Planter;
    }
    if !sid.starts_with("S-1-5-21-") {
        return Owner::Unresolved("a well-known SID set-up doesn't judge".to_owned());
    }
    let local = match local {
        Ok(local) => local,
        Err(why) => return Owner::Unresolved(format!("this machine's domain: {why}")),
    };
    if security::account_domain(sid).as_ref() != Some(local) {
        return Owner::Unresolved("not a local account".to_owned());
    }
    match tree.in_administrators(sid) {
        Ok(true) => Owner::OtherAdmin,
        Ok(false) => Owner::Planter,
        Err(why) => Owner::Unresolved(why),
    }
}

/// Who owned a key before the take-over.
enum Owner {
    /// SYSTEM or Administrators.
    Admin,
    /// An account in Administrators.
    OtherAdmin,
    /// A SID ordinary users hold, or an account not in Administrators.
    Planter,
    /// An account whose membership couldn't be told, with why.
    Unresolved(String),
    /// `TrustedInstaller`, `LocalService`, `NetworkService` or another service SID.
    Service,
}

/// Deletes `key` and everything under it, bottom up through handles. Each child is taken over
/// (for `DELETE`) before its own subkeys are read; one that is gone is done.
fn drain<T: Tree>(tree: &mut T, key: T::Key) -> Result<(), VendorFault> {
    let names = unless_gone(tree.subkeys(&key), VendorFault::CleanUp)?.unwrap_or_default();
    let mut stack = vec![(key, VecDeque::from(names))];
    while let Some((parent, names)) = stack.last_mut() {
        let Some(name) = names.pop_front() else {
            if let Some((done, _)) = stack.pop() {
                unless_gone(tree.delete(done), VendorFault::CleanUp)?;
            }
            continue;
        };
        let Some((child, _)) = unless_gone(
            tree.take_over_child(parent, &name, LOCKED_SDDL),
            VendorFault::Secure,
        )?
        else {
            continue;
        };
        let names = unless_gone(tree.subkeys(&child), VendorFault::CleanUp)?.unwrap_or_default();
        stack.push((child, names.into()));
    }
    Ok(())
}
