use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::{Os, RECEIPT, SCRIPT, Tool, command, exit_byte, find_python_with, runtime_program};

/// The files a test's file system holds: a runtime's receipt and interpreter under `root`.
fn runtime(root: &Path, os: Os) -> BTreeSet<PathBuf> {
    BTreeSet::from([root.join(RECEIPT), runtime_program(root, os)])
}

fn find(var: Option<&str>, dirs: &[&Path], os: Os, files: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    let path = (!dirs.is_empty()).then(|| std::env::join_paths(dirs).unwrap());
    find_python_with(var.map(OsStr::new), path.as_deref(), os, &|file| {
        files.contains(file)
    })
}

fn root() -> PathBuf {
    std::env::temp_dir().join("clauseo-python-3.13.18-c3-test")
}

#[test]
fn the_variable_wins_when_absolute_and_a_relative_or_empty_one_counts_as_unset() {
    let files = runtime(&root(), Os::Unix);
    let bin = root().join("bin");
    let var = std::env::temp_dir().join("python-x");
    assert_eq!(find(var.to_str(), &[&bin], Os::Unix, &files), Some(var));
    assert_eq!(find(Some("python3"), &[&bin], Os::Unix, &files), None);
    assert_eq!(
        find(Some(""), &[&bin], Os::Unix, &files),
        Some(bin.join("python3"))
    );
}

#[test]
fn the_macos_runtime_is_found_from_its_bin_folder_after_other_folders() {
    let files = runtime(&root(), Os::Unix);
    let other = std::env::temp_dir().join("other");
    let bin = root().join("bin");
    assert_eq!(
        find(None, &[&other, &bin], Os::Unix, &files),
        Some(bin.join("python3"))
    );
}

#[test]
fn the_windows_runtime_is_found_from_its_root_or_its_command_folder() {
    let files = runtime(&root(), Os::Windows);
    for dir in [root().join("clauseo-bin"), root()] {
        let found = find(None, &[&dir], Os::Windows, &files);
        assert_eq!(found, Some(root().join("python.exe")), "{}", dir.display());
    }
}

#[test]
fn no_runtime_without_a_receipt_an_interpreter_an_absolute_folder_or_a_path() {
    let bin = root().join("bin");
    let no_receipt = BTreeSet::from([runtime_program(&root(), Os::Unix)]);
    assert_eq!(find(None, &[&bin], Os::Unix, &no_receipt), None);
    let no_program = BTreeSet::from([root().join(RECEIPT)]);
    assert_eq!(find(None, &[&bin], Os::Unix, &no_program), None);
    let files = runtime(&root(), Os::Unix);
    assert_eq!(find(None, &[], Os::Unix, &files), None);
    let relative = BTreeSet::from([
        PathBuf::from("rt").join(RECEIPT),
        runtime_program(Path::new("rt"), Os::Unix),
    ]);
    assert_eq!(
        find(None, &[&Path::new("rt").join("bin")], Os::Unix, &relative),
        None
    );
}

#[test]
fn the_command_runs_the_script_isolated_with_the_tool_and_arguments_as_given() {
    let args: Vec<OsString> = ["-jpeg", "-r", "100", "a b.pdf", "-"]
        .into_iter()
        .map(OsString::from)
        .collect();
    let python = Path::new("/rt/bin/python3");
    let command = command(python, Tool::Pdftoppm, &args);
    assert_eq!(command.get_program(), python.as_os_str());
    let got: Vec<&OsStr> = command.get_args().collect();
    let mut want: Vec<&OsStr> = ["-I", "-X", "utf8", "-c", SCRIPT, "pdftoppm"]
        .into_iter()
        .map(OsStr::new)
        .collect();
    want.extend(args.iter().map(OsString::as_os_str));
    assert_eq!(got, want);
    let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
    assert_eq!(
        envs,
        [
            (OsStr::new("PYTHONHOME"), None),
            (OsStr::new("PYTHONPATH"), None)
        ]
    );
    let info = super::command(python, Tool::Pdfinfo, &[]);
    assert_eq!(info.get_args().nth(5), Some(OsStr::new("pdfinfo")));
}

#[test]
fn the_script_fits_a_windows_command_line() {
    // CreateProcess takes at most 32,767 characters; the script goes as one argument.
    assert!(SCRIPT.len() < 16_000, "{} bytes", SCRIPT.len());
}

#[test]
fn an_exit_code_past_a_byte_or_none_is_poppler_other_error() {
    assert_eq!(exit_byte(Some(0)), 0);
    assert_eq!(exit_byte(Some(1)), 1);
    assert_eq!(exit_byte(Some(99)), 99);
    assert_eq!(exit_byte(Some(-1_073_741_819)), 99);
    assert_eq!(exit_byte(None), 99);
}
