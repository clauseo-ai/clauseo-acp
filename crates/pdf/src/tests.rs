use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::{
    Os, PDFTOTEXT, RECEIPT, SCRIPT, Tool, command, exit_byte, find_python_with, runtime_program,
};

/// The files a test's file system holds: a runtime's receipt and interpreter under `root`.
fn runtime(root: &Path, os: Os) -> BTreeSet<PathBuf> {
    BTreeSet::from([root.join(RECEIPT), runtime_program(root, os)])
}

fn find(dirs: &[&Path], os: Os, files: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    let path = (!dirs.is_empty()).then(|| std::env::join_paths(dirs).unwrap());
    find_python_with(path.as_deref(), os, &|file| files.contains(file))
}

fn root() -> PathBuf {
    std::env::temp_dir().join("clauseo-python-3.13.18-c3-test")
}

#[test]
fn the_macos_runtime_is_found_from_its_bin_folder_after_other_folders() {
    let files = runtime(&root(), Os::Unix);
    let other = std::env::temp_dir().join("other");
    let bin = root().join("bin");
    assert_eq!(
        find(&[&other, &bin], Os::Unix, &files),
        Some(bin.join("python3"))
    );
}

#[test]
fn the_windows_runtime_is_found_from_its_root_or_its_command_folder() {
    let files = runtime(&root(), Os::Windows);
    for dir in [root().join("clauseo-bin"), root()] {
        let found = find(&[&dir], Os::Windows, &files);
        assert_eq!(found, Some(root().join("python.exe")), "{}", dir.display());
    }
}

#[test]
fn no_runtime_without_a_receipt_an_interpreter_an_absolute_folder_or_a_path() {
    let bin = root().join("bin");
    let no_receipt = BTreeSet::from([runtime_program(&root(), Os::Unix)]);
    assert_eq!(find(&[&bin], Os::Unix, &no_receipt), None);
    let no_program = BTreeSet::from([root().join(RECEIPT)]);
    assert_eq!(find(&[&bin], Os::Unix, &no_program), None);
    let files = runtime(&root(), Os::Unix);
    assert_eq!(find(&[], Os::Unix, &files), None);
    let relative = BTreeSet::from([
        PathBuf::from("rt").join(RECEIPT),
        runtime_program(Path::new("rt"), Os::Unix),
    ]);
    assert_eq!(
        find(&[&Path::new("rt").join("bin")], Os::Unix, &relative),
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
    let text = super::command(python, Tool::Pdftotext, &[]);
    let args: Vec<&OsStr> = text.get_args().skip(4).collect();
    assert_eq!(args, [OsStr::new(PDFTOTEXT), OsStr::new("pdftotext")]);
}

#[test]
fn the_script_fits_a_windows_command_line() {
    // CreateProcess takes at most 32,767 characters; the script goes as one argument.
    assert!(escaped(SCRIPT) < 24_000, "{} characters", escaped(SCRIPT));
}

#[test]
fn the_pdftotext_script_fits_a_windows_command_line() {
    // pdftotext's own arguments are a few flags and at most two paths, so its script may take
    // more of the line than pdftoppm's, whose arguments are as many: 30,000 leaves 2,767 for the
    // interpreter's path, the flags and two file paths.
    assert!(
        escaped(PDFTOTEXT) < 30_000,
        "{} characters",
        escaped(PDFTOTEXT)
    );
}

/// A script's length as one Windows command-line argument: each `"` and `\` escaped.
fn escaped(script: &str) -> usize {
    script.len() + script.matches(['"', '\\']).count()
}

#[test]
fn an_exit_code_past_a_byte_or_none_is_poppler_other_error() {
    assert_eq!(exit_byte(Some(0)), 0);
    assert_eq!(exit_byte(Some(1)), 1);
    assert_eq!(exit_byte(Some(99)), 99);
    assert_eq!(exit_byte(Some(-1_073_741_819)), 99);
    assert_eq!(exit_byte(None), 99);
}
