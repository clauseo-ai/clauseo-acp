use super::{Agent, Grammar, NotPlain, Place, WRAPPERS, plain};

const MAC: Place<'static> = Place {
    // unix-paths-ok: a Place is compared as text by `plain` and never reaches a path API
    matter: "/Users/l/Matters/Acme",
    cwd: "/Users/l/Matters/Acme", // unix-paths-ok: the same text, compared as text
    windows: false,
};
const WIN: Place<'static> = Place {
    matter: r"C:\Users\l\Matters\Acme",
    cwd: r"C:\Users\l\Matters\Acme",
    windows: true,
};

fn claude(line: &str) -> Result<Vec<String>, NotPlain> {
    plain(line, Grammar::Posix, Agent::Claude, MAC)
}

fn codex(line: &str) -> Result<Vec<String>, NotPlain> {
    plain(line, Grammar::Posix, Agent::Codex, MAC)
}

fn ps(line: &str) -> Result<Vec<String>, NotPlain> {
    plain(line, Grammar::PowerShell, Agent::Claude, WIN)
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
    assert_eq!(claude("pdftotext a.pdf out.txt").unwrap().len(), 3);
    assert_eq!(claude("ls   -1\tDrafts").unwrap(), ["ls", "-1", "Drafts"]);
    assert_eq!(
        claude("cp a.txt b'c'd.txt").unwrap(),
        ["cp", "a.txt", "bcd.txt"]
    );
    assert_eq!(claude("echo 'a|b;c'").unwrap(), ["echo", "a|b;c"]);
    assert_eq!(claude("echo \"a|b\"").unwrap(), ["echo", "a|b"]);
    assert_eq!(claude("ls Écrits").unwrap(), ["ls", "Écrits"]);
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
        "echo $HOME",
        "echo \"$HOME\"",
        "echo \"`ls`\"",
        "echo \"a\\\"b\"",
        "ls *.pdf",
        "ls a?.pdf",
        "ls [ab].pdf",
        "ls {a,b}.pdf",
        "ls ~/x",
        "ls # c",
        "echo !1",
        "ls ^a",
        "echo 'open",
        "echo \"open",
        "echo a\\ b",
        "(ls)",
        "{ ls; }",
        "echo \"a!\"",
        "ls\u{200b}",
        "ls 'a\u{202e}b'",
        "ls\u{a0}a",
        "echo 'a\u{7}'",
    ] {
        assert_eq!(claude(line), Err(NotPlain::Unparsed), "{line:?}");
    }
}

#[test]
fn every_wrapper_on_the_list_is_refused() {
    for program in WRAPPERS {
        let line = format!("{program} x");
        let got = claude(&line);
        assert!(got.is_err(), "{line:?} was kept");
    }
}

#[test]
fn wrappers_in_any_spelling_are_refused() {
    for line in [
        "bash -c ls",
        "sh -c 'ls'",
        "zsh -lc ls",
        "fish -c ls",
        "python3 x.py",
        "python3.12 x.py",
        "Python x.py",
        "node x.js",
        "ruby x.rb",
        "perl -e 1",
        "osascript -e 1",
        "env ls",
        "sudo ls",
        "xargs rm",
        "nohup ls",
        "time ls",
        "nice ls",
        "timeout 5 ls",
        "open a.pdf",
        "swift read.swift",
        "npx x",
        "make",
        "git status",
        "curl https://a.b",
        "pwsh -c ls",
        "cmd /c dir",
        "powershell.exe -Command ls",
        "PYTHON.EXE x.py",
        "awk 1 a.txt",
        "sed -n 1p a.txt",
    ] {
        assert_eq!(claude(line), Err(NotPlain::Wrapper), "{line:?}");
    }
}

#[test]
fn a_program_with_a_folder_or_a_variable_is_refused() {
    for line in ["./run.sh", "/bin/ls", "bin/tool a", "A=1 ls", "-x a"] {
        assert_eq!(claude(line), Err(NotPlain::Unparsed), "{line:?}");
    }
}

#[test]
fn find_and_tar_with_a_program_or_a_delete_are_refused() {
    for line in [
        "find . -exec rm {} ;",
        "find . -name a -delete",
        "find . -execdir ls",
        "find . -ok ls",
        "find . -fprint out",
        "tar --to-command=sh -xf a.tar",
        "tar -I zstd -xf a.tar",
        "tar --checkpoint-action=exec=sh -xf a.tar",
    ] {
        assert!(claude(line).is_err(), "{line:?}");
    }
    assert!(claude("find . -name a.pdf").is_ok());
    assert!(claude("tar -xf a.tar").is_ok());
}

#[test]
fn an_argument_outside_the_matter_is_refused() {
    for line in [
        "cat /etc/passwd",
        "mv Q ../Q2",
        "cat Drafts/../../B/a.txt",
        "cp a.txt --to=/tmp/a",
        "ls /Users/l/Matters/Acme2",
    ] {
        assert_eq!(claude(line), Err(NotPlain::Outside), "{line:?}");
    }
    assert!(claude("cat /Users/l/Matters/Acme/a.txt").is_ok());
    assert!(claude("cat Drafts/../a.txt").is_ok());
    assert!(claude("ls /Users/l/Matters/Acme").is_ok());
}

#[test]
fn codex_s_own_wrapper_is_read_through_once() {
    // codex-acp strips `/bin/zsh -lc '…'` itself; any other shell path is left as typed.
    assert_eq!(codex("ls -la").unwrap(), ["ls", "-la"]);
    assert_eq!(codex("bash -lc 'ls -la'").unwrap(), ["ls", "-la"]);
    assert_eq!(codex("zsh -c ls").unwrap(), ["ls"]);
    assert_eq!(codex("bash -lc 'ls | head'"), Err(NotPlain::Unparsed));
    assert_eq!(codex("bash -lc 'python3 x.py'"), Err(NotPlain::Wrapper));
    assert_eq!(codex("bash -lc \"bash -c 'ls'\""), Err(NotPlain::Wrapper));
    assert_eq!(codex("bash -lc ls extra"), Err(NotPlain::Wrapper));
    assert_eq!(codex("fish -c ls"), Err(NotPlain::Wrapper));
    // Codex's own shell may be in any folder (codex-acp strips only `/bin/`).
    // unix-paths-ok: command text only; `codex` parses it and no path API opens it
    assert_eq!(codex("/opt/homebrew/bin/bash -lc ls").unwrap(), ["ls"]);
    // Claude's own shell takes the script as given: a `bash -c` in it is the model's.
    assert_eq!(claude("bash -lc 'ls -la'"), Err(NotPlain::Wrapper));
}

#[test]
fn codex_s_powershell_wrapper_is_read_as_powershell() {
    let win = |line| plain(line, Grammar::Posix, Agent::Codex, WIN);
    assert_eq!(
        win("powershell.exe -NoProfile -Command 'Get-ChildItem Drafts'").unwrap(),
        ["Get-ChildItem", "Drafts"]
    );
    assert_eq!(
        win("pwsh -Command 'Get-ChildItem; Remove-Item a'"),
        Err(NotPlain::Unparsed)
    );
    assert_eq!(
        win("powershell -ExecutionPolicy Bypass -Command Get-ChildItem"),
        Err(NotPlain::Wrapper)
    );
}

#[test]
fn powershell_takes_few_forms() {
    assert_eq!(
        ps(r"Get-Content .\Drafts\a.txt").unwrap(),
        ["Get-Content", r".\Drafts\a.txt"]
    );
    assert_eq!(
        ps("Get-ChildItem 'My Files'").unwrap(),
        ["Get-ChildItem", "My Files"]
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
        "ls -Filter *.pdf",
        "Get-ChildItem 'it''s'",
        "ls a'b'",
        "ls 'a'b",
    ] {
        assert_eq!(ps(line), Err(NotPlain::Unparsed), "{line:?}");
    }
    for line in [
        "iex a",
        "Invoke-Expression a",
        "START-PROCESS notepad",
        "Set-Location ..",
        "Invoke-WebRequest x",
    ] {
        assert_eq!(ps(line), Err(NotPlain::Wrapper), "{line:?}");
    }
    assert_eq!(
        ps(r"Get-Content C:\Windows\win.ini"),
        Err(NotPlain::Outside)
    );
    assert_eq!(
        ps(r"Get-Content \\server\share\a.txt"),
        Err(NotPlain::Outside)
    );
    assert!(ps(r"Get-Content c:\users\L\matters\acme\a.txt").is_ok());
}

#[test]
fn an_empty_or_long_command_is_refused() {
    assert_eq!(claude(""), Err(NotPlain::Size));
    assert_eq!(claude("   "), Err(NotPlain::Unparsed));
    let long = format!("ls {}", "a".repeat(super::MAX_COMMAND_BYTES));
    assert_eq!(claude(&long), Err(NotPlain::Size));
}
