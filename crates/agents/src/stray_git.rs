//! A git program in a Codex install's package folders, removed at every start. The host refuses
//! a Windows Codex launch while a git program ([`clauseo_process::WINDOWS_GIT_PROGRAMS`]) sits
//! in any `vendor/<triple>` folder or a folder in it (`clauseo-host`'s
//! `agent/no_git/codex_install.rs`). So after `sac::settle`, each start removes such a file there
//! ([`remove`]), read-only or not, and a restart clears that refusal. No link is followed; a file
//! that stays is logged, and the host keeps refusing. On every OS (the names never occur in
//! Codex's packages).

use std::path::{Path, PathBuf};

use clauseo_process::WINDOWS_GIT_PROGRAMS;

use crate::write;

/// Removes every git program from `folder`'s Codex packages' `vendor/<triple>` folders and the
/// folders in them; returns how many went. Logs a count, never a path.
pub(crate) fn remove(folder: &Path) -> usize {
    let modules = folder.join("node_modules");
    let packages = modules.join("@openai");
    let vendors: Vec<PathBuf> = real_folders(&packages)
        .into_iter()
        .map(|package| package.join("vendor"))
        .collect();
    // A link at any of these levels: nothing in the install is touched.
    if [&modules, &packages]
        .into_iter()
        .chain(&vendors)
        .any(|path| is_link(path))
    {
        tracing::warn!("a Codex install folder is a link: no git program removed from it");
        return 0;
    }
    let mut places = Vec::new();
    for vendor in &vendors {
        for triple in real_folders(vendor) {
            places.extend(real_folders(&triple));
            places.push(triple);
        }
    }
    let (mut removed, mut stayed) = (0_usize, 0_usize);
    for place in places {
        for entry in std::fs::read_dir(&place).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let git = WINDOWS_GIT_PROGRAMS
                .iter()
                .any(|program| name.eq_ignore_ascii_case(program));
            if !git || entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            match write::remove_file_forced(&entry.path()) {
                Ok(()) => removed += 1,
                Err(_) => stayed += 1,
            }
        }
    }
    if removed + stayed > 0 {
        tracing::warn!(
            removed,
            stayed,
            "git programs found in Codex's install folders at start"
        );
    }
    removed
}

/// Whether `path` itself is a link or a junction (never followed).
fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.file_type().is_symlink() || write::is_reparse_point(&meta))
}

/// The real folders directly in `dir` (no link, no junction), none when it can't be read.
fn real_folders(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && entry
                    .metadata()
                    .is_ok_and(|meta| !write::is_reparse_point(&meta))
        })
        .map(|entry| entry.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::write;

    fn file(path: &Path) {
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_atomic(path, b"x").unwrap();
    }

    #[test]
    fn a_git_program_in_codex_vendor_folders_is_removed_and_the_rest_stays() {
        let root =
            std::env::temp_dir().join(format!("clauseo-agents-stray-git-{}", std::process::id()));
        drop(write::remove_dir_all(&root));
        let triple = root
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor")
            .join("x86_64-pc-windows-msvc");
        let keep = [
            triple.join("bin").join("codex.exe"),
            triple.join("codex-path").join("rg.exe"),
        ];
        let gone = [
            triple.join("bin").join("git.exe"),
            triple.join("codex-path").join("GIT.CMD"),
            triple.join("git.com"),
        ];
        for path in keep.iter().chain(&gone) {
            file(path);
        }
        assert_eq!(super::remove(&root), 3);
        assert!(keep.iter().all(|path| path.is_file()));
        assert!(gone.iter().all(|path| !path.exists()));
        assert_eq!(super::remove(&root), 0);
        drop(write::remove_dir_all(&root));
    }

    /// An install whose `vendor` and a git program two levels inside the folder it leads to.
    fn linked_vendor(test: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "clauseo-agents-stray-git-{test}-{}",
            std::process::id()
        ));
        drop(write::remove_dir_all(&root));
        let outside = root.join("outside");
        let git = outside
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("git.exe");
        file(&git);
        let package = root
            .join("install")
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64");
        write::create_dir_all(&package).unwrap();
        (root, outside, git)
    }

    /// A `vendor` that is a link: nothing under it is removed.
    #[cfg(unix)]
    #[test]
    fn a_linked_vendor_folder_is_left_alone() {
        let (root, outside, git) = linked_vendor("link");
        let vendor = root
            .join("install")
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor");
        write::symlink(&outside, &vendor).unwrap();
        assert_eq!(super::remove(&root.join("install")), 0);
        assert!(git.is_file());
        drop(write::remove_dir_all(&root));
    }

    /// The same with a junction at `vendor`.
    #[cfg(windows)]
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the test makes its junction with cmd's mklink /J, which exits at once"
    )]
    fn a_vendor_junction_is_left_alone() {
        let (root, outside, git) = linked_vendor("junction");
        let vendor = root
            .join("install")
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor");
        let made = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&vendor)
            .arg(&outside)
            .status()
            .unwrap();
        assert!(made.success());
        assert_eq!(super::remove(&root.join("install")), 0);
        assert!(git.is_file());
        drop(write::remove_dir_all(&root));
    }
}
