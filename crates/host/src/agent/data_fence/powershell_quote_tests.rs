//! PowerShell's own quoting in the data folder fence's reading of a command line
//! (`command.rs`'s `split_line`, which on Windows returns the shared reader's words,
//! `pre_write_guard`'s `powershell_words`): both of its callers, the line's own simple commands
//! and a script handed to `pwsh -Command`, read `'C:\…\O''Brien & Sons\brief.txt'` as the
//! folder `O'Brien & Sons`, not `OBrien & Sons` (#1165's review). Each path checked here is
//! another matter's or the data folder's, so a misreading would let a read there through.

use std::path::PathBuf;

use super::ShellKind;
use super::command::{command_paths, recursive_roots};

fn has(paths: &[PathBuf], want: &str) -> bool {
    paths.iter().any(|path| path == &PathBuf::from(want))
}

#[test]
fn an_apostrophe_doubled_in_single_quotes_is_one_apostrophe() {
    let straight = r"C:\Matters\O'Brien & Sons\brief.txt";
    let curly = "C:\\Matters\\O\u{2019}Brien & Sons\\brief.txt";
    for (line, want) in [
        (
            r"Get-Content 'C:\Matters\O''Brien & Sons\brief.txt'",
            straight,
        ),
        (
            r#"Get-Content "C:\Matters\O'Brien & Sons\brief.txt""#,
            straight,
        ),
        (
            "Get-Content 'C:\\Matters\\O\u{2019}\u{2019}Brien & Sons\\brief.txt'",
            curly,
        ),
        // The script a `pwsh -Command` runs, read one level down (the second caller).
        (
            r#"pwsh -Command "Get-Content 'C:\Matters\O''Brien & Sons\brief.txt'""#,
            straight,
        ),
    ] {
        let paths = command_paths(line, true);
        assert!(has(&paths, want), "{line}: {paths:?}");
    }
    // A walker's folder, through the same reader.
    let roots = recursive_roots(
        r"Get-ChildItem -Recurse 'C:\Matters\O''Brien & Sons'",
        true,
        ShellKind::Unknown,
    );
    assert!(has(&roots, r"C:\Matters\O'Brien & Sons"), "{roots:?}");
}

/// CR, LF and CRLF each end a line, and a comment (observed on Windows PowerShell 5.1 and
/// PowerShell 7.4, VM, 16 Nov 2026): a path after a lone CR is still read.
#[test]
fn a_lone_cr_ends_a_comment_and_the_path_after_it_is_read() {
    let paths = command_paths("# note\rGet-Content C:\\Data\\secret.txt", true);
    assert!(has(&paths, r"C:\Data\secret.txt"), "{paths:?}");
}

/// A here-string's body is one value, but a path written inside a body of more than one line is
/// still read, as the fence's own reader read it before (a one-line body is one word, as then).
#[test]
fn a_path_inside_a_here_string_is_read() {
    let line = "$s = @'\nType C:\\Data\\secret.txt\nnow, O'Brien\n'@; Invoke-Expression $s";
    let paths = command_paths(line, true);
    assert!(has(&paths, r"C:\Data\secret.txt"), "{paths:?}");
}
