//! Windows' system names (round 3b): their values come from what the OS said, on every OS.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_settings::KnownFolder;

use super::{Native, PROGRAM_FOLDERS, SYSTEM_NAMES, arch_name, system_vars};

/// The program folders a fake machine names; every other known folder is none.
const FOLDERS: [(KnownFolder, &str); 7] = [
    (KnownFolder::ProgramData, "C:\\ProgramData"),
    (KnownFolder::ProgramFiles, "C:\\Program Files"),
    (KnownFolder::ProgramFilesX64, "C:\\Program Files"),
    (KnownFolder::ProgramFilesX86, "C:\\Program Files (x86)"),
    (
        KnownFolder::ProgramFilesCommon,
        "C:\\Program Files\\Common Files",
    ),
    (
        KnownFolder::ProgramFilesCommonX64,
        "C:\\Program Files\\Common Files",
    ),
    (
        KnownFolder::ProgramFilesCommonX86,
        "C:\\Program Files (x86)\\Common Files",
    ),
];

fn program_folder(known: KnownFolder) -> Option<PathBuf> {
    FOLDERS
        .iter()
        .find(|(folder, _)| *folder == known)
        .map(|(_, path)| PathBuf::from(path))
}

fn text(vars: Vec<(OsString, OsString)>) -> BTreeMap<String, String> {
    vars.into_iter()
        .map(|(name, value)| (name.into_string().unwrap(), value.into_string().unwrap()))
        .collect()
}

#[test]
fn every_system_name_comes_from_what_the_os_said() {
    let (vars, unset) = system_vars(&Native {
        root: Some(PathBuf::from("D:\\Windows")),
        folder: &program_folder,
        arch: Some(12),
        processors: Some(8),
    });
    assert_eq!(unset, 0);
    let names: Vec<String> = vars
        .iter()
        .map(|(name, _)| name.clone().into_string().unwrap())
        .collect();
    assert_eq!(names, SYSTEM_NAMES);
    let vars = text(vars);
    let comspec = PathBuf::from("D:\\Windows")
        .join("System32")
        .join("cmd.exe");
    assert_eq!(vars["ComSpec"], comspec.to_str().unwrap());
    assert_eq!(vars["SystemDrive"], "D:");
    for (name, known) in PROGRAM_FOLDERS {
        assert_eq!(
            vars[name],
            program_folder(known).unwrap().to_str().unwrap(),
            "{name}"
        );
    }
    assert_eq!(vars["PROCESSOR_ARCHITECTURE"], "ARM64");
    assert_eq!(vars["NUMBER_OF_PROCESSORS"], "8");
    assert_eq!(vars["OS"], "Windows_NT");
    // Every name is a host-set name on the allow-list, so nothing else can give it a value.
    for name in SYSTEM_NAMES {
        assert_eq!(
            crate::agent::jailed_env::allowed(std::ffi::OsStr::new(name)),
            Some(crate::agent::jailed_env::Rule::Host),
            "{name}"
        );
    }
}

/// A value the OS does not give leaves its name unset and counted: never a fallback.
#[test]
fn a_name_the_os_gives_no_value_for_is_unset_and_counted() {
    let (vars, unset) = system_vars(&Native {
        root: None,
        folder: &|_| None,
        arch: Some(0xFFFF),
        processors: Some(0),
    });
    assert_eq!(unset, SYSTEM_NAMES.len() - 1);
    assert_eq!(
        text(vars),
        BTreeMap::from([("OS".to_owned(), "Windows_NT".to_owned())])
    );
    // A Windows folder with no drive letter gives `ComSpec` but no `SystemDrive`.
    let (vars, _) = system_vars(&Native {
        root: Some(PathBuf::from("\\\\server\\Windows")),
        folder: &|_| None,
        arch: None,
        processors: None,
    });
    let vars = text(vars);
    assert!(vars.contains_key("ComSpec"));
    assert!(!vars.contains_key("SystemDrive"));
}

#[test]
fn the_architecture_words_are_windows_own() {
    for (arch, word) in [(0, "x86"), (5, "ARM"), (9, "AMD64"), (12, "ARM64")] {
        assert_eq!(arch_name(arch), Some(word));
    }
    assert_eq!(arch_name(6), None);
    assert_eq!(arch_name(0xFFFF), None);
}

/// On Windows the real machine gives every name.
#[cfg(windows)]
#[test]
fn this_windows_machine_gives_every_system_name() {
    let (vars, unset) = super::native();
    assert_eq!(unset, 0);
    let vars = text(vars);
    let root = crate::agent::jailed_env::system_root().unwrap();
    assert_eq!(
        vars["ComSpec"],
        root.join("System32").join("cmd.exe").to_str().unwrap()
    );
    assert!(std::path::Path::new(&vars["ComSpec"]).is_file());
    assert!(std::path::Path::new(&vars["ProgramFiles"]).is_dir());
    assert!(vars["NUMBER_OF_PROCESSORS"].parse::<u32>().unwrap() > 0);
}
