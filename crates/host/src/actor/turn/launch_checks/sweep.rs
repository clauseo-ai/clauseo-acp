//! The start-up sweep of `<data>/run` ([`RunSweep`]). See `launch_checks.rs`.

use std::path::Path;

use super::{remove_tree, run_dir};

/// The start-up sweep of `<data>/run`, once per host start: no launch is live before the
/// first, so every folder there is an earlier run's leftover. The first launch-prep thread, under
/// a lock every launch's prep takes before making its folders, makes the parking folder
/// `<data>/set-aside` and renames `run` into it (`run-<n>`) in one call, so no live folder is
/// taken and no launch waits on a delete; a `clauseo-run-sweep` thread then deletes every folder
/// parked there (round 6). A `run` that is a link is left as it is, and the launches refuse it.
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

/// The prefix of a `run` folder set aside at the data folder's top before the parking folder
/// (legacy): moved into the parking folder at the next start.
const LEGACY: &str = "run.old-";

/// Makes the parking folder (`state_folder::set_aside_dir`, through no link below `data`), moves
/// any legacy `run.old-*` folder into it, and renames `<data>/run`, when it is a real folder, to
/// the first free `run-<n>` there: each in one rename on the same volume.
fn set_aside(data: &Path) {
    let parking = crate::agent::state_folder::set_aside_dir(data);
    if let Err(error) = crate::write::create_private_dirs_under(data, &parking) {
        tracing::warn!(%error, "the parking folder could not be made; the earlier run stays");
        return;
    }
    let free = |stem: &str| {
        (0..10_000_u32)
            .map(|n| parking.join(format!("{stem}-{n}")))
            .find(|parked| std::fs::symlink_metadata(parked).is_err())
    };
    if let Ok(entries) = std::fs::read_dir(data) {
        for entry in entries.flatten() {
            let legacy = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(LEGACY));
            if legacy
                && crate::write::is_real_dir(&entry.path())
                && let Some(parked) = free("legacy")
                && let Err(error) = crate::write::rename_folder(&entry.path(), &parked)
            {
                tracing::warn!(%error, "a legacy set-aside folder stays");
            }
        }
    }
    let run = run_dir(data);
    match std::fs::symlink_metadata(&run) {
        Err(_) => return,
        Ok(meta) if !meta.is_dir() => {
            tracing::warn!("the run folder is a link or no folder; left as is");
            return;
        }
        Ok(_) => {}
    }
    let renamed = match free("run") {
        Some(parked) => crate::write::rename_folder(&run, &parked),
        None => Err(std::io::Error::other(
            "no free run-<n> name in the parking folder",
        )),
    };
    if let Err(error) = renamed {
        tracing::warn!(%error, "the earlier run's folders stay");
    }
}

/// Deletes every real folder in the parking folder (the earlier run, legacy `run.old-*`, and
/// state-folder trees set aside as too deep), and any legacy `run.old-*` folder left at the
/// data folder's top; a link is left alone. The parking folder itself stays.
fn delete_set_aside(data: &Path) {
    let parking = crate::agent::state_folder::set_aside_dir(data);
    if crate::write::is_real_dir(&parking)
        && let Ok(entries) = std::fs::read_dir(&parking)
    {
        for entry in entries.flatten() {
            if crate::write::is_real_dir(&entry.path()) {
                remove_tree(&entry.path());
            }
        }
    }
    let Ok(entries) = std::fs::read_dir(data) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let named = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(LEGACY));
        if named && crate::write::is_real_dir(&path) {
            remove_tree(&path);
        }
    }
}
