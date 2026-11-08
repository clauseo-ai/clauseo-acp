//! The fake's moves: a rename's keys, and the copy and link of a walked tree.

use std::collections::BTreeMap;
use std::path::Path;

use super::{FakeTrees, Node, key, under};
use crate::windows::git_tree::{Entry, Refused};

/// The file at `path` must be the walk's `entry`: a link is refused, another inode changed.
pub(super) fn same_file(fake: &FakeTrees, path: &Path, entry: &Entry) -> Result<u64, Refused> {
    match fake.node(path) {
        Some(Node::File(inode)) if u128::from(*inode) == entry.id => Ok(*inode),
        Some(Node::Link) => Err(Refused::Reparse),
        Some(Node::File(_)) => Err(Refused::Changed),
        _ => Err(Refused::Other(format!("{} isn't a file", key(path)))),
    }
}

/// Moves every key at or below `from` to the same place below `to`.
pub(super) fn move_keys(fake: &mut FakeTrees, from: &str, to: &str) {
    rekey(&mut fake.nodes, from, to);
    rekey(&mut fake.meta, from, to);
    rekey(&mut fake.texts, from, to);
    rekey(&mut fake.marker_sddls, from, to);
    let held: Vec<String> = fake
        .in_use
        .iter()
        .filter(|at| under(at, from))
        .cloned()
        .collect();
    for at in held {
        fake.in_use.remove(&at);
        fake.in_use
            .insert(format!("{to}{}", at.get(from.len()..).unwrap_or_default()));
    }
}

/// [`TreeFs::copy_tree`] and [`TreeFs::link_tree`]: `link` links, else copies.
pub(super) fn put_tree(
    fake: &mut FakeTrees,
    from: &Path,
    entries: &[Entry],
    to: &Path,
    link: bool,
) -> Result<(), Refused> {
    let other = Refused::Other;
    for entry in entries {
        let (source, target) = (from.join(&entry.rel), to.join(&entry.rel));
        if entry.folder {
            fake.call(format!("make_dir {}", key(&target)))
                .map_err(other)?;
            fake.parent_folder(&target).map_err(other)?;
            fake.nodes.insert(key(&target), Node::Folder(None));
            continue;
        }
        let op = if link { "hard_link" } else { "copy_file" };
        fake.call(format!("{op} {} -> {}", key(&source), key(&target)))
            .map_err(other)?;
        if let Some(replaced) = fake.replace_on_copy.take() {
            fake.new_file(&replaced);
        }
        let inode = same_file(fake, &source, entry)?;
        fake.parent_folder(&target).map_err(other)?;
        if fake.node(&target).is_some() {
            return Err(other(format!("{} is there", key(&target))));
        }
        if link {
            fake.nodes.insert(key(&target), Node::File(inode));
        } else {
            fake.new_file(&key(&target));
        }
    }
    Ok(())
}

/// Moves the keys of `map` at or below `from` to the same place below `to`.
fn rekey<V>(map: &mut BTreeMap<String, V>, from: &str, to: &str) {
    let keys: Vec<String> = map.keys().filter(|at| under(at, from)).cloned().collect();
    for at in keys {
        if let Some(value) = map.remove(&at) {
            map.insert(
                format!("{to}{}", at.get(from.len()..).unwrap_or_default()),
                value,
            );
        }
    }
}
