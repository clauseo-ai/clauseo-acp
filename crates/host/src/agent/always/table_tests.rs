//! The allowlist (`table.rs`).

use super::{At, PROGRAMS, Unfit, find, path_word, read};
use crate::agent::always::plain::Grammar;

fn args(line: &str) -> Vec<String> {
    line.split(' ').map(str::to_owned).collect()
}

fn paths(program: &str, grammar: Grammar, line: &str) -> Result<Vec<String>, Unfit> {
    let spec = find(program, grammar).unwrap();
    let args = if line.is_empty() {
        Vec::new()
    } else {
        args(line)
    };
    read(spec, &args).map(|read| read.paths)
}

fn posix(program: &str, line: &str) -> Result<Vec<String>, Unfit> {
    paths(program, Grammar::Posix, line)
}

/// Every entry is one the reader can hold to: a bare name, options spelled as options, flags
/// that are single letters, a sane operand count, and only Clauseo's own tool bundled.
#[test]
fn every_entry_is_well_made() {
    for spec in PROGRAMS {
        assert!(
            !spec.name.is_empty() && !spec.name.contains(['/', '\\', ' ']),
            "{}",
            spec.name
        );
        assert!(spec.count.0 <= spec.count.1, "{}", spec.name);
        assert!(
            spec.flags.chars().all(|c| c.is_ascii_alphanumeric()),
            "{}",
            spec.name
        );
        for option in spec.options {
            assert!(
                option.name.starts_with('-') && !option.name.contains('='),
                "{}",
                spec.name
            );
        }
        assert_eq!(
            spec.at == At::Bundled,
            spec.name == "stetdoc",
            "{}",
            spec.name
        );
        assert_eq!(
            spec.at == At::Cmdlet,
            spec.grammar == Grammar::PowerShell,
            "{}",
            spec.name
        );
        assert_eq!(
            find(spec.name, spec.grammar).map(|found| found.name),
            Some(spec.name)
        );
    }
}

/// What is never on the list: programs that run code, reach the network or drive a browser,
/// ones that delete, link or run another program, and any that writes.
#[test]
fn programs_that_run_code_or_reach_out_are_not_listed() {
    for name in [
        "python3",
        "python",
        "swift",
        "node",
        "osascript",
        "curl",
        "wget",
        "agent-browser",
        "bash",
        "sh",
        "zsh",
        "env",
        "xargs",
        "find",
        "rm",
        "ln",
        "tar",
        "rg",
        "sort",
        "sed",
        "awk",
        "open",
        "git",
        "cupsfilter",
        "pandoc",
        "soffice",
        "cp",
        "mv",
        "mkdir",
        "touch",
        "tee",
    ] {
        assert!(find(name, Grammar::Posix).is_none(), "{name}");
    }
    for name in [
        "Invoke-Expression",
        "Start-Process",
        "Remove-Item",
        "New-Item",
        "Set-Content",
        "Copy-Item",
        "Move-Item",
        "Out-File",
    ] {
        assert!(find(name, Grammar::PowerShell).is_none(), "{name}");
    }
    assert!(find("get-content", Grammar::PowerShell).is_some());
    assert!(
        find("LS", Grammar::Posix).is_none(),
        "POSIX names are exact"
    );
    assert!(find("Get-Content", Grammar::Posix).is_none());
}

#[test]
fn a_command_s_words_are_read_against_its_entry() {
    assert_eq!(posix("ls", "").unwrap(), Vec::<String>::new());
    assert_eq!(posix("ls", "-la Drafts").unwrap(), ["Drafts"]);
    assert_eq!(posix("head", "-n 5 a.txt").unwrap(), ["a.txt"]);
    assert_eq!(
        posix("grep", "-n -i term a.txt b.txt").unwrap(),
        ["a.txt", "b.txt"]
    );
    assert_eq!(posix("grep", "-in term a.txt").unwrap(), ["a.txt"]);
    assert_eq!(
        posix("mdls", "-name kMDItemNumberOfPages a.pdf").unwrap(),
        ["a.pdf"]
    );
    assert_eq!(posix("stetdoc", "view a.docx").unwrap(), ["a.docx"]);
    assert_eq!(
        posix("stetdoc", "view --revisions a.docx").unwrap(),
        ["a.docx"]
    );
    assert_eq!(posix("stetdoc", "--help").unwrap(), Vec::<String>::new());
    assert_eq!(
        posix("stetdoc", "view --help").unwrap(),
        Vec::<String>::new()
    );
    let ps = |program, line| paths(program, Grammar::PowerShell, line);
    assert_eq!(
        ps("Get-Content", "-Path a.txt -TotalCount 5").unwrap(),
        ["a.txt"]
    );
    assert_eq!(ps("Get-Content", "-path a.txt").unwrap(), ["a.txt"]);
    assert_eq!(
        ps("Select-String", "term a.txt").unwrap(),
        ["a.txt"],
        "the pattern first, then the file"
    );
    assert_eq!(
        ps("Select-String", "-Pattern term -Path a.txt").unwrap(),
        ["a.txt"]
    );
}

#[test]
fn words_that_do_not_fit_are_refused() {
    let cases = [
        ("head", "-n x a.txt", Unfit::Word),
        ("head", "-n -5 a.txt", Unfit::Word),
        ("head", "-n", Unfit::Option),
        ("head", "-n5 a.txt", Unfit::Option),
        ("head", "--lines=5 a.txt", Unfit::Option),
        ("grep", "-e term a.txt", Unfit::Option),
        ("grep", "-f/etc/x term a.txt", Unfit::Option),
        ("grep", "--include=x term a.txt", Unfit::Option),
        ("grep", "term", Unfit::Count),
        ("grep", "-x", Unfit::Count),
        ("cat", "", Unfit::Count),
        ("cat", "-", Unfit::Word),
        ("cat", "=zsh", Unfit::Word),
        ("cat", "--a=b=/abs", Unfit::Option),
        ("cat", "a/../../b", Unfit::Word),
        ("ls", "-o../x", Unfit::Option),
        ("stetdoc", "edit a.docx", Unfit::Count),
        ("stetdoc", "", Unfit::Count),
        ("stetdoc", "--revisions", Unfit::Count),
    ];
    for (program, line, why) in cases {
        assert_eq!(posix(program, line), Err(why), "{program} {line}");
    }
    let ps = |program, line| paths(program, Grammar::PowerShell, line);
    assert_eq!(ps("Get-Content", "-Path:C:/x"), Err(Unfit::Option));
    assert_eq!(ps("Get-Content", "FileSystem::C:/x"), Err(Unfit::Word));
    assert_eq!(ps("Get-Content", "HKCU:/Software"), Err(Unfit::Word));
    assert_eq!(ps("Get-Content", "a.txt b.txt"), Err(Unfit::Count));
    assert_eq!(ps("Get-ChildItem", "-Recurse a"), Err(Unfit::Option));
    assert_eq!(
        ps("Get-ChildItem", "-rf"),
        Err(Unfit::Option),
        "no joined flags in PowerShell"
    );
}

#[test]
fn a_path_word_is_a_plain_path() {
    for word in [
        "a.txt",
        "Drafts/a b.txt",
        "C:/m/a.txt",
        "/m/a",
        ".",
        "./a",
        "Écrits/x",
    ] {
        assert!(path_word(word), "{word}");
    }
    for word in [
        "",
        "-a",
        "=zsh",
        "a=b",
        "HKCU:/x",
        "FileSystem::C:/x",
        "C:x",
        "~/x",
        "a\\b",
        "..",
        "../x",
        "a/../b",
        "a/..",
        "C:/../x",
    ] {
        assert!(!path_word(word), "{word}");
    }
}

/// BSD `cat`, `head`, `ls` and `wc` stop reading options at the first operand: a word starting
/// with `-` after it is a file, so it is refused rather than read as an option.
#[test]
fn an_option_after_the_first_operand_is_refused() {
    for (program, line) in [
        ("cat", "a -n"),
        ("cat", "notes.txt -n"),
        ("head", "notes.txt -n 5"),
        ("ls", "Drafts -la"),
        ("wc", "a.txt -l"),
        ("grep", "term a.txt -n"),
    ] {
        assert_eq!(posix(program, line), Err(Unfit::Option), "{program} {line}");
    }
    assert_eq!(posix("cat", "-n a").unwrap(), ["a"]);
    assert_eq!(
        posix("stetdoc", "view a.docx --revisions"),
        Err(Unfit::Option)
    );
}

/// With `-Pattern` given by name, Select-String's operands are all paths (PowerShell binds the
/// first to `-Path`).
#[test]
fn select_string_with_a_named_pattern_reads_its_operands_as_paths() {
    let ps = |line| paths("Select-String", Grammar::PowerShell, line);
    assert_eq!(ps("-Pattern x a.txt").unwrap(), ["a.txt"]);
    assert_eq!(ps("a.txt -Pattern x").unwrap(), ["a.txt"]);
    assert_eq!(ps("x a.txt").unwrap(), ["a.txt"]);
}

/// No recursive search or listing: `grep -r` and `ls -R` would read the matter's `.clauseo` and
/// `.claude` folders.
#[test]
fn no_search_or_listing_walks_the_matter() {
    for (program, line) in [
        ("grep", "-r term ."),
        ("grep", "-rn term Drafts"),
        ("ls", "-R"),
        ("ls", "-laR ."),
    ] {
        assert_eq!(posix(program, line), Err(Unfit::Option), "{program} {line}");
    }
}
