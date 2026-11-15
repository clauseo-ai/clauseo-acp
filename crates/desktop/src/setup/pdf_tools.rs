//! Clauseo's own `pdftoppm` and `pdfinfo` (the `clauseo-pdf` crate: launchers that run Clauseo's
//! Python runtime's `pypdfium2`, no poppler), bundled in a folder of their own:
//! `Clauseo.app/Contents/Helpers/pdf/` on macOS, `<resources>/pdf/` elsewhere (on Windows the
//! install folder; `.github/scripts/sidecars.mjs`, `PDF_TOOLS`). Claude Code's Read renders long
//! PDFs with them and Codex is told to; the folder goes on both agents' session `PATH` with the
//! Python runtime's folders, and only while those are there (`SetupHook::agent_path`): the
//! programs need that runtime, and find it on the same `PATH`.

use std::path::{Path, PathBuf};

/// The folder's name, under `Contents/Helpers` or the resource folder.
pub(crate) const FOLDER: &str = "pdf";

/// The programs the folder holds (with the OS's executable suffix).
pub(crate) const PROGRAMS: [&str; 2] = ["pdftoppm", "pdfinfo"];

/// Where a release bundles the folder: on macOS (`macos`) `Contents/Helpers/pdf`, found from
/// `exe_dir` (`Contents/MacOS`), as stetdoc's (`profile::bundled_stetdoc`); elsewhere
/// `<resource_dir>/pdf`. None when that folder is not known.
pub(crate) fn bundled_folder(
    macos: bool,
    exe_dir: &Path,
    resource_dir: Option<&Path>,
) -> Option<PathBuf> {
    let parent = if macos {
        exe_dir.parent()?.join("Helpers")
    } else {
        resource_dir?.to_path_buf()
    };
    Some(parent.join(FOLDER))
}

/// Whether `folder` holds both [`PROGRAMS`] as programs.
pub(crate) fn usable(folder: &Path) -> bool {
    PROGRAMS.iter().all(|name| {
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        crate::profile::is_program(&folder.join(file))
    })
}

/// The bundled folder for an app whose executable is in `exe_dir` ([`bundled_folder`]), when it
/// holds both programs; one log line either way, with no path. A debug build has none. Reads
/// the disk: on the host-start thread (`DesktopProfile::build_in`).
pub(crate) fn found(macos: bool, exe_dir: &Path, resource_dir: Option<&Path>) -> Option<PathBuf> {
    let folder = bundled_folder(macos, exe_dir, resource_dir)?;
    let found = usable(&folder);
    tracing::info!(
        found,
        "Clauseo's pdftoppm and pdfinfo: with Python, on the agents' PATH only when found"
    );
    found.then_some(folder)
}

#[cfg(test)]
mod tests;
