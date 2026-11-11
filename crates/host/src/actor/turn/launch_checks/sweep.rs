//! The start-up sweep of `<data>/run` ([`RunSweep`]). See `launch_checks.rs`.

use std::path::Path;

use super::{browser_run_dir, remove_tree, run_dir};

/// The start-up sweep of `<data>/run`, once per host start: no launch is live before the
/// first, so every folder there is an earlier run's leftover. The first launch-prep thread, under
/// a lock every launch's prep takes before making its folders, renames `run` to
/// `run.old-<n>` in one call, so no live folder is taken and no launch waits on a delete; a
/// `clauseo-run-sweep` thread then deletes every `run.old-*` folder. A `run` that is a link
/// is left as it is, and the launches refuse it. `<data>/browser/run`, the sessions' browser
/// folders (the guard's `--cwd`), is set aside and deleted the same way, within `<data>/browser`.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunSweep(std::sync::Arc<std::sync::Mutex<Swept>>);

/// Whether the start-up sweep ran, and (for the tests) its delete's thread.
#[derive(Debug, Default)]
struct Swept {
    done: bool,
    #[cfg(test)]
    delete: Option<std::thread::JoinHandle<()>>,
}

impl RunSweep {
    /// Sets `<data>/run` aside the first time and starts its delete; nothing after.
    pub(crate) fn once(&self, data: &Path) {
        let mut swept = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if swept.done {
            return;
        }
        swept.done = true;
        let parents = [data.to_owned(), data.join(crate::browser::CONFIG_DIR)];
        set_aside(&run_dir(data), &parents[0]);
        set_aside(&browser_run_dir(data), &parents[1]);
        let started = std::thread::Builder::new()
            .name("clauseo-run-sweep".to_owned())
            .spawn(move || parents.iter().for_each(|parent| delete_set_aside(parent)));
        match started {
            #[cfg(test)]
            Ok(delete) => swept.delete = Some(delete),
            #[cfg(not(test))]
            Ok(_delete) => {}
            Err(error) => {
                tracing::warn!(%error, "the earlier run's folders stay until the next start");
            }
        }
    }

    /// Waits for the start-up sweep's delete, when it started.
    #[cfg(test)]
    pub(crate) fn join_delete(&self) {
        let delete = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .delete
            .take();
        if let Some(delete) = delete {
            delete.join().unwrap();
        }
    }
}

/// The prefix of a `run` folder set aside.
const SET_ASIDE: &str = "run.old-";

/// Renames `run` (`<data>/run` or `<data>/browser/run`) to the first free `run.old-<n>` in
/// `parent`, when it is a real folder.
fn set_aside(run: &Path, parent: &Path) {
    match std::fs::symlink_metadata(run) {
        Err(_) => return,
        Ok(meta) if !meta.is_dir() => {
            tracing::warn!("the run folder is a link or no folder; left as is");
            return;
        }
        Ok(_) => {}
    }
    let free = (0..1000_u32)
        .map(|n| parent.join(format!("{SET_ASIDE}{n}")))
        .find(|old| std::fs::symlink_metadata(old).is_err());
    let renamed = match free {
        Some(old) => crate::write::rename_folder(run, &old),
        None => Err(std::io::Error::other("no free run.old-<n> name")),
    };
    if let Err(error) = renamed {
        tracing::warn!(%error, "the earlier run's folders stay");
    }
}

/// Deletes every real `run.old-*` folder in `parent`, and the short temp folder each of its
/// sessions names (`agent::claude_tmp`); a link of that name is left alone.
fn delete_set_aside(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let named = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(SET_ASIDE));
        if named && crate::write::is_real_dir(&path) {
            // A crashed session's short temp folder for Claude goes with its run folder.
            for session in std::fs::read_dir(&path).into_iter().flatten().flatten() {
                crate::agent::claude_tmp::remove_claude_tmp(&session.path());
            }
            remove_tree(&path);
        }
    }
}
