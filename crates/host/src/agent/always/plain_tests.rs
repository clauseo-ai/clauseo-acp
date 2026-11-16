use super::{Agent, Grammar, NotPlain, Place, plain};

const MAC: Place<'static> = Place {
    // unix-paths-ok: a Place is compared as text by `plain` and never reaches a path API
    matter: "/Users/l/Matters/Acme",
    windows: false,
};
const WIN: Place<'static> = Place {
    matter: "C:/Users/l/Matters/Acme",
    windows: true,
};

fn words(got: Result<super::Plain, NotPlain>) -> Result<Vec<String>, NotPlain> {
    got.map(|plain| plain.words)
}

fn claude(line: &str) -> Result<Vec<String>, NotPlain> {
    words(plain(line, Grammar::Posix, Agent::Claude, MAC))
}

fn codex(line: &str) -> Result<Vec<String>, NotPlain> {
    words(plain(line, Grammar::Posix, Agent::Codex, MAC))
}

fn ps(line: &str) -> Result<Vec<String>, NotPlain> {
    words(plain(line, Grammar::PowerShell, Agent::Claude, WIN))
}

#[test]
fn a_plain_command_is_its_words() {
    assert_eq!(
        claude("mdls -name kMDItemTitle Brief.pdf").unwrap(),
        ["mdls", "-name", "kMDItemTitle", "Brief.pdf"]
    );
    assert_eq!(
        claude("ls -la 'My File.docx' \"Other File.pdf\"").unwrap(),
        ["ls", "-la", "My File.docx", "Other File.pdf"]
    );
    assert_eq!(claude("ls   -1\tDrafts").unwrap(), ["ls", "-1", "Drafts"]);
    assert_eq!(
        claude("cat a.txt b'c'd.txt").unwrap(),
        ["cat", "a.txt", "bcd.txt"]
    );
    let got = plain("grep -n term a.txt", Grammar::Posix, Agent::Claude, MAC).unwrap();
    assert_eq!(got.program.name, "grep");
    assert_eq!(got.paths, ["a.txt"]);
}

#[test]
fn chains_pipes_redirections_and_substitutions_are_refused() {
    for line in [
        "ls && rm a",
        "ls || rm a",
        "ls; rm a",
        "ls | head",
        "ls > out.txt",
        "ls >> out.txt",
        "ls 2> err.txt",
        "cat < a.txt",
        "cat $(ls)",
        "cat `ls`",
        "sleep 1 &",
        "ls\nrm a",
        "ls\r\nrm a",
        "cat $HOME",
        "cat \"$HOME\"",
        "cat \"`ls`\"",
        "cat \"a\\\"b\"",
        "cat 'a|b;c'",
        "ls *.pdf",
        "ls a?.pdf",
        "ls [ab].pdf",
        "ls {a,b}.pdf",
        "ls ~/x",
        "ls # c",
        "cat !1",
        "ls ^a",
        "cat 'open",
        "cat \"open",
        "cat a\\ b",
        "(ls)",
        "{ ls; }",
        "cat \"a!\"",
        "ls\u{200b}",
        "ls 'a\u{202e}b'",
        "ls\u{a0}a",
        "cat 'a\u{7}'",
    ] {
        assert_eq!(claude(line), Err(NotPlain::Unparsed), "{line:?}");
    }
}

/// Only programs the allowlist names (`table.rs`); nothing is let in by not being named.
#[test]
fn a_program_off_the_list_is_refused() {
    for line in [
        "python3 x.py",
        "python3 -c 1",
        "swift -e 1",
        "curl -sI https://a.in",
        "agent-browser eval document.title",
        "cupsfilter a.txt",
        "bash -c ls",
        "sh x",
        "env ls",
        "sudo ls",
        "xargs rm",
        "rm a.txt",
        "ln -s a b",
        "find . -name a.pdf",
        "tar -xf a.tar",
        "rg term",
        "sort -o out a",
        "sed -n 1p a.txt",
        "awk 1 a.txt",
        "git status",
        "open a.pdf",
        "osascript -e 1",
        "pwd",
        "echo a",
        "LS",
        "Get-Content a.txt",
    ] {
        assert_eq!(claude(line), Err(NotPlain::NotListed), "{line:?}");
    }
}

/// Runners and programs that run code from a file the agent writes or a flag names: none is on
/// the list, so none is kept, whichever program they run.
#[test]
fn runners_and_programs_that_run_a_file_are_refused() {
    for line in [
        "xcrun swift brief.swift",
        "arch -arm64 python3 x.py",
        "sandbox-exec -n x python3 x.py",
        "latexmk -pdf brief.tex",
        "rg --pre ./pre pattern",
        "fd -x ./t",
        "sort --compress-program=./z f",
        "sort -o/Users/l/.zshrc notes.txt",
        "zip -T -TT ./t a.zip f",
        "pandoc --lua-filter=f.lua in.md -o out.docx",
        "openssl x -engine ./x.so",
        "setsid x",
        "ssh-agent x",
        "xvfb-run x",
        "unshare x",
        "firejail x",
        "control x.cpl",
        "hh x.chm",
        "pcalua -a x",
        "conhost x",
        "cmstp x",
        "installutil x",
        "regasm x",
        "msdt x",
    ] {
        assert_eq!(claude(line), Err(NotPlain::NotListed), "{line:?}");
        assert_eq!(codex(line), Err(NotPlain::NotListed), "{line:?}");
    }
}

#[test]
fn a_program_with_a_folder_or_a_variable_is_refused() {
    for line in [
        "./ls",
        "/bin/ls",
        "bin/ls a",
        "A=1 ls",
        "-x a",
        ".ls",
        "./bash -c ls",
        "x/sh -c ls",
        // unix-paths-ok: command text only, parsed and never opened
        "/Users/l/Matters/Acme/bash -c ls",
    ] {
        assert_eq!(claude(line), Err(NotPlain::Unparsed), "{line:?}");
        assert!(codex(line).is_err(), "{line:?}");
    }
}

/// An option the program's entry does not list, or a value in a form it does not know, is
/// refused: no joined value, no `--name=value` for an option it does not list, no provider path.
#[test]
fn an_option_off_the_list_is_refused() {
    for line in [
        "head -n5 a.txt",
        "head -n -5 a.txt",
        "head -n",
        "grep -f/etc/x term a.txt",
        "grep -e term a.txt",
        "grep --include=x term a.txt",
        "grep -R term Drafts",
        "cat --a=b=/abs",
        "ls -o../x",
        "cp -t /tmp a.txt",
        "cp --target-directory=/tmp a.txt",
        "mdls -name kMDItemTitle -nullMarker x a.pdf",
        "cat =zsh",
        "cat a=b",
        "cat ../x",
        "cat Drafts/../../B/a.txt",
        "cat -",
        "cp a.txt",
        "stetdoc edit a.docx",
        "stetdoc",
    ] {
        assert_eq!(claude(line), Err(NotPlain::NotListed), "{line:?}");
    }
    for line in [
        "Copy-Item a.txt -Destination:C:/x",
        "Get-Content FileSystem::C:/x",
        "Get-Content HKCU:/Software",
        "Get-Content C:x",
        "Copy-Item -Recurse a b",
        "Get-Content a.txt -Encoding x",
    ] {
        assert_eq!(ps(line), Err(NotPlain::NotListed), "{line:?}");
    }
}

#[test]
fn an_argument_outside_the_matter_is_refused() {
    for line in [
        "cat /etc/passwd",
        "ls /Users/l/Matters/Acme2",
        "cat /Users/l/Q /Users/l/Q2",
        "grep term /etc",
    ] {
        assert_eq!(claude(line), Err(NotPlain::Outside), "{line:?}");
    }
    assert!(claude("cat /Users/l/Matters/Acme/a.txt").is_ok());
    assert!(claude("ls /Users/l/Matters/Acme").is_ok());
    assert!(claude("mdls -name kMDItemNumberOfPages /Users/l/Matters/Acme/a.pdf").is_ok());
}

#[test]
fn codex_s_own_wrapper_is_read_through_only_at_its_system_path() {
    // codex-acp shows `/bin/zsh -lc 'script'` as the script alone; when it does not, the wrapper
    // is read through only at the paths Codex launches.
    assert_eq!(codex("ls -la").unwrap(), ["ls", "-la"]);
    assert_eq!(codex("/bin/zsh -lc 'ls -la'").unwrap(), ["ls", "-la"]);
    assert_eq!(codex("/bin/bash -c ls").unwrap(), ["ls"]);
    assert_eq!(
        codex("/bin/sh -c 'wc -l a.txt'").unwrap(),
        ["wc", "-l", "a.txt"]
    );
    assert_eq!(codex("/bin/zsh -lc 'ls | head'"), Err(NotPlain::Unparsed));
    assert_eq!(
        codex("/bin/zsh -lc 'python3 x.py'"),
        Err(NotPlain::NotListed)
    );
    assert_eq!(
        codex("/bin/zsh -lc \"/bin/bash -c ls\""),
        Err(NotPlain::Unparsed)
    );
    assert_eq!(codex("/bin/zsh -lc ls extra"), Err(NotPlain::Unparsed));
    assert_eq!(codex("/bin/zsh -x ls"), Err(NotPlain::Unparsed));
    assert_eq!(codex("bash -lc ls"), Err(NotPlain::NotListed));
    assert_eq!(codex("zsh -c ls"), Err(NotPlain::NotListed));
    // unix-paths-ok: command text only; `codex` parses it and no path API opens it
    let homebrew = "/opt/homebrew/bin/bash -lc ls";
    assert_eq!(codex(homebrew), Err(NotPlain::Unparsed));
    // Claude's own shell takes the script as given: a `bash -c` in it is the model's.
    assert_eq!(claude("/bin/bash -lc 'ls -la'"), Err(NotPlain::Unparsed));
}

#[test]
fn codex_s_powershell_wrapper_is_read_as_powershell() {
    let win = |line| words(plain(line, Grammar::PowerShell, Agent::Codex, WIN));
    assert_eq!(
        win(
            "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -Command 'Get-ChildItem Drafts'"
        )
        .unwrap(),
        ["Get-ChildItem", "Drafts"]
    );
    for line in [
        "powershell.exe -NoProfile -Command 'Get-ChildItem Drafts'",
        "pwsh -Command 'Get-ChildItem'",
        "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -ExecutionPolicy Bypass -Command Get-ChildItem",
        "C:/Users/l/Matters/Acme/powershell.exe -Command Get-ChildItem",
    ] {
        assert!(win(line).is_err(), "{line:?}");
    }
}

#[test]
fn powershell_takes_few_forms() {
    assert_eq!(
        ps("Get-Content Drafts/a.txt").unwrap(),
        ["Get-Content", "Drafts/a.txt"]
    );
    assert_eq!(
        ps("get-childitem 'My Files'").unwrap(),
        ["get-childitem", "My Files"]
    );
    for line in [
        "Get-ChildItem | Remove-Item",
        "Get-Content $env:USERPROFILE",
        "Get-Content (ls)",
        "ls; rm a",
        "Get-ChildItem \"a b\"",
        "Get-ChildItem `n",
        "@(1)",
        "Get-ChildItem > a",
        "& notepad",
        "Get-ChildItem -Filter *.pdf",
        "Get-ChildItem 'it''s'",
        "Get-ChildItem a'b'",
        "Get-ChildItem 'a'b",
        "Get-Content .\\Drafts\\a.txt",
        "Get-Content a,b",
        "Get-Content @args",
        // A typographic quote ends a PowerShell string as `'` does.
        "Get-Content '\u{2019}; . ./x.ps1; \u{2019}'",
        "Get-Content \u{2018}a\u{2019}",
        "Get-Content '\u{201c}a\u{201d}'",
    ] {
        assert_eq!(ps(line), Err(NotPlain::Unparsed), "{line:?}");
    }
    for line in [
        "iex a",
        "Invoke-Expression a",
        "START-PROCESS notepad",
        "Set-Location ..",
        "Invoke-WebRequest x",
        "Remove-Item a",
        "New-Item a",
    ] {
        assert_eq!(ps(line), Err(NotPlain::NotListed), "{line:?}");
    }
    assert_eq!(ps("Get-Content C:/Windows/win.ini"), Err(NotPlain::Outside));
    assert_eq!(
        ps("Get-Content //server/share/a.txt"),
        Err(NotPlain::Outside)
    );
    assert!(ps("Get-Content c:/users/L/matters/acme/a.txt").is_ok());
}

#[test]
fn an_empty_or_long_command_is_refused() {
    assert_eq!(claude(""), Err(NotPlain::Size));
    assert_eq!(claude("   "), Err(NotPlain::Unparsed));
    let long = format!("ls {}", "a".repeat(super::MAX_COMMAND_BYTES));
    assert_eq!(claude(&long), Err(NotPlain::Size));
}

/// A command that writes is never kept, by either agent, in either shell: no program on the list
/// writes (`cp`, `mv`, `mkdir`, `Copy-Item`, `Move-Item`).
#[test]
fn no_command_that_writes_is_kept() {
    for line in [
        "cp memo.txt Drafts",
        "cp -r Exhibits Bundle",
        "cp -R Template/ .",
        "mv Pack Drafts",
        "mkdir Drafts",
    ] {
        assert_eq!(claude(line), Err(NotPlain::NotListed), "{line:?}");
        assert_eq!(codex(line), Err(NotPlain::NotListed), "{line:?}");
    }
    for line in ["Copy-Item a.txt Drafts", "Move-Item Pack Drafts"] {
        assert_eq!(ps(line), Err(NotPlain::NotListed), "{line:?}");
    }
    assert!(claude("cat memo.txt").is_ok(), "a read is still kept");
    assert!(ps("Get-Content memo.txt").is_ok(), "a read is still kept");
}

/// A POSIX option after the first file name is a file to BSD `cat`, `cp` and the rest: refused.
#[test]
fn an_option_after_a_file_name_is_refused() {
    for line in [
        "cat a -n",
        "cp a b -R",
        "head notes.txt -n 5",
        "ls Drafts -la",
    ] {
        assert_eq!(claude(line), Err(NotPlain::NotListed), "{line:?}");
    }
}

/// PowerShell reads a bare word that looks like a number as one (`1kb` is 1024), and a quoted
/// word starting with `-` as a file, never a switch: both refused. A plain number still fits.
#[test]
fn powershell_numbers_and_quoted_dashes_are_refused() {
    for line in [
        "Get-Content 1kb",
        "Get-Content 0x10",
        "Get-Content 1e3",
        "Get-Content 10l",
        "Get-Content 007",
        "Get-Content +5",
        "Get-Content .5",
        "Get-Content 1234567890",
        "Get-Content '-Raw'",
        "Get-Content -Path '-x'",
        "Select-String '-SimpleMatch' a.txt",
    ] {
        assert_eq!(ps(line), Err(NotPlain::Unparsed), "{line:?}");
    }
    assert!(ps("Get-Content -TotalCount 10 a.txt").is_ok());
    assert!(ps("Get-Content '1kb'").is_ok(), "a quoted name is text");
    assert!(ps("Get-Content ./1kb").is_ok());
    assert!(
        claude("head -n 007 a.txt").is_ok(),
        "POSIX reads digits as they are"
    );
}

/// No letter outside ASCII, so no look-alike name reaches a card.
#[test]
fn a_letter_outside_ascii_is_refused() {
    for line in ["ls Écrits", "cat 'Écrits/a.txt'", "cat а.txt"] {
        assert_eq!(claude(line), Err(NotPlain::Unparsed), "{line:?}");
    }
}
