//! The start-up sweep of `<data>/run` ([`RunSweep`]). See `launch_checks.rs`.

use std::path::Path;

use super::{remove_tree, run_dir};

/// The start-up sweep of `<data>/run`, once per host start: no launch is live before the
/// first, so every folder there is an earlier run's leftover. The first launch-prep thread, under
/// a lock every launch's prep takes before making its folders, renames `run` to
/// `run.old-<n>` in one call, so no live folder is taken and no launch waits on a delete; a
/// `clauseo-run-sweep` thread then deletes every `run.old-*` folder. A `run` that is a link
/// is left as it is, and the launches refuse it.
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
        set_aside(data);
        let data = data.to_owned();
        let started = std::thread::Builder::new()
            .name("clauseo-run-sweep".to_owned())
            .spawn(move || delete_set_aside(&data));
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

/// Renames `<data>/run` to the first free `run.old-<n>`, when it is a real folder.
fn set_aside(data: &Path) {
    let run = run_dir(data);
    match std::fs::symlink_metadata(&run) {
        Err(_) => return,
        Ok(meta) if !meta.is_dir() => {
            tracing::warn!("the run folder is a link or no folder; left as is");
            return;
        }
        Ok(_) => {}
    }
    let free = (0..1000_u32)
        .map(|n| data.join(format!("{SET_ASIDE}{n}")))
        .find(|old| std::fs::symlink_metadata(old).is_err());
    let renamed = match free {
        Some(old) => crate::write::rename_folder(&run, &old),
        None => Err(std::io::Error::other("no free run.old-<n> name")),
    };
    if let Err(error) = renamed {
        tracing::warn!(%error, "the earlier run's folders stay");
    }
}

/// Deletes every real `run.old-*` folder under `data`; a link of that name is left alone.
fn delete_set_aside(data: &Path) {
    let Ok(entries) = std::fs::read_dir(data) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let named = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(SET_ASIDE));
        if named && crate::write::is_real_dir(&path) {
            remove_tree(&path);
        }
    }
}
