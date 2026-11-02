//! Review round 2: the name backstop. A shell command naming a held file of the matter is refused
//! whatever folder games it plays, unless it only reads; and `stetdoc`'s forms.

use serde_json::{Value, json};

use super::{Fixture, deny, guard_json, guard_with};
use crate::write;

fn bash(command: &str) -> Value {
    json!({"command": command})
}

/// What the guard says for `command` under `tool`, the matter as `cwd`, Codex's way (no
/// `--matter`); the same must come with Claude Code's `--matter`.
pub(super) fn check(fx: &Fixture, tool: &str, command: &str) -> String {
    let hook = fx.hook(tool, &bash(command));
    let without = guard_json(&hook);
    let matter = fx.path("m");
    let with = guard_with(&["--matter", &matter], hook.to_string().as_bytes());
    assert_eq!(without, with, "{command}");
    without
}

pub(super) fn add(fx: &Fixture, names: &[&str]) {
    for name in names {
        let path = name
            .split('/')
            .fold(fx.matter(), |acc, part| acc.join(part));
        if let Some(parent) = path.parent() {
            write::create_dir_all(parent).unwrap();
        }
        write::write_file(&path, b"x").unwrap();
    }
}

/// Each command of the round-2 re-check reaches a held file by a route the folder tracking
/// misses or used to miss; each names it, so the backstop refuses it.
#[test]
fn every_round_2_probe_is_refused() {
    let fx = Fixture::new("backstop-probes");
    add(
        &fx,
        &["Drafts/deeper/x", "Drafts/Only.docx", "Drafts/~$Only.docx"],
    );
    let home = std::env::var("HOME").unwrap_or_default();
    let from_home = fx
        .path("m/Contract.docx")
        .strip_prefix(&format!("{home}/"))
        .map_or_else(|| "m/Contract.docx".to_owned(), str::to_owned);
    let only = deny("Drafts/Only.docx");
    let contract = deny("Contract.docx");
    for (tool, command, refusal) in [
        (
            "Bash",
            "cd a; cd ..; cd b; cd ..; cd c; cd ..; cd d; cd ..; cd Drafts/deeper; echo hi > ../Only.docx"
                .to_owned(),
            &only,
        ),
        ("Bash", format!("cd ~ && echo hi > {from_home}"), &contract),
        ("Bash", "cd && echo hi > m/Contract.docx".to_owned(), &contract),
        (
            "Bash",
            "cd \"$(dirname \"$PWD\")\" && echo hi > m/Contract.docx".to_owned(),
            &contract,
        ),
        (
            "PowerShell",
            "<# note #> Set-Content Contract.docx hi".to_owned(),
            &contract,
        ),
        (
            "PowerShell",
            "cd Drafts\\deeper; cd..; Set-Content Only.docx hi".to_owned(),
            &only,
        ),
        (
            "PowerShell",
            "cd..; Set-Content m\\Contract.docx hi".to_owned(),
            &contract,
        ),
        ("Bash", "echo hi > $'Con\\x74ract.docx'".to_owned(), &contract),
        ("Bash", "echo hi > $'Con\\164ract.docx'".to_owned(), &contract),
        (
            "Bash",
            "timeout 30 bash -c 'cd Drafts && echo hi > ../Contract.docx'".to_owned(),
            &contract,
        ),
        (
            "Bash",
            "env bash -c 'cd Drafts && echo hi > ../Contract.docx'".to_owned(),
            &contract,
        ),
        (
            "Bash",
            "nohup sh -c 'cd Drafts && echo hi > ../Contract.docx'".to_owned(),
            &contract,
        ),
        (
            "Bash",
            "echo \"$(cd Drafts && echo hi > ../Contract.docx)\"".to_owned(),
            &contract,
        ),
        (
            "Bash",
            "echo $((1<<2))\ncd Drafts\necho hi > ../Contract.docx".to_owned(),
            &contract,
        ),
    ] {
        assert_eq!(&check(&fx, tool, &command), refusal, "{command}");
    }
}

/// A different file, or a name that only contains the held one, is let through.
#[test]
fn a_command_not_naming_a_held_file_as_a_whole_name_is_let_through() {
    let fx = Fixture::new("backstop-free");
    for command in [
        "echo hi > Free.docx",
        "cp Contract.docx.bak x.docx",
        "echo hi > MyContract.docx",
        "cd Drafts && echo hi > ../Free.docx",
    ] {
        assert_eq!(check(&fx, "Bash", command), "", "{command}");
    }
}

/// The name compares as the file system does: ignoring case on macOS and Windows.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_held_name_in_another_case_is_refused() {
    let fx = Fixture::new("backstop-case");
    assert_eq!(
        check(&fx, "Bash", "cd x; echo hi > ../CONTRACT.DOCX"),
        deny("Contract.docx")
    );
}

/// Elsewhere another case is another file.
#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn a_held_name_in_another_case_is_another_file() {
    let fx = Fixture::new("backstop-case");
    assert_eq!(check(&fx, "Bash", "cd x; echo hi > ../CONTRACT.DOCX"), "");
}

/// Reading a held document through the shell is let through: the lawyer keeps it open in Word
/// while asking about it.
#[test]
fn reading_a_held_file_is_let_through() {
    let fx = Fixture::new("backstop-reads");
    for command in [
        "cat Contract.docx",
        "head -c 100 Contract.docx",
        "tail -n 5 ./Contract.docx",
        "wc -c Contract.docx",
        "file Contract.docx",
        "strings Contract.docx | head",
        "unzip -l Contract.docx",
        "unzip -p Contract.docx word/document.xml | head -c 2000",
        "textutil -convert txt -stdout Contract.docx",
        "pandoc Contract.docx -t plain",
        "pandoc Contract.docx -o summary.md",
        "pandoc --output=out.txt Contract.docx",
        "cat Contract.docx | grep -c x",
        "cat Contract.docx 2>/dev/null",
        "cat Contract.docx > copy.txt 2>&1",
        "cd sub && cat ../Contract.docx",
        "sh -c 'cat Contract.docx'",
        "time cat Contract.docx",
        "stetdoc view Contract.docx --outline",
    ] {
        assert_eq!(check(&fx, "Bash", command), "", "{command}");
    }
    assert_eq!(check(&fx, "PowerShell", "cat Contract.docx"), "");
}

/// Writing, or running code that names the held file, is refused.
#[test]
fn writing_a_held_file_through_the_shell_is_refused() {
    let fx = Fixture::new("backstop-writes");
    let contract = deny("Contract.docx");
    for command in [
        "cat Contract.docx > Contract.docx",
        "cat x >> Contract.docx",
        "cat x >| Contract.docx",
        "cat x &> Contract.docx",
        "cat x 2> Contract.docx",
        "cat Contract.docx > $out",
        "cat x | tee Contract.docx",
        "cp x Contract.docx",
        "mv Contract.docx y",
        "rm Contract.docx",
        "dd if=x of=Contract.docx",
        "truncate -s 0 Contract.docx",
        "sed -i s/a/b/ Contract.docx",
        "perl -i -pe s/a/b/ Contract.docx",
        "tee Contract.docx",
        "touch Contract.docx",
        "ln -sf x Contract.docx",
        "echo Contract.docx",
        "python -c \"open('Contract.docx','w')\"",
        "python3 -c \"open('Contract.docx','w')\"",
        "node -e \"require('fs').writeFileSync('Contract.docx','')\"",
        "ruby -e \"File.write('Contract.docx','')\"",
        "perl -e \"open(F,'>Contract.docx')\"",
        "osascript -e 'tell app \"Word\" to save document \"Contract.docx\"'",
        "sh -c 'echo hi > Contract.docx'",
        "bash -c 'cat Contract.docx > Contract.docx'",
        "cat \"$(echo Contract.docx)\"",
        "pandoc x.md -o Contract.docx",
        "pandoc x.md --output=Contract.docx",
        "python3 <<EOF\nopen('Contract.docx','w')\nEOF",
        "less -o Contract.docx x",
        "unzip Contract.docx",
        "textutil -convert txt Contract.docx",
    ] {
        assert_eq!(check(&fx, "Bash", command), contract, "{command}");
    }
    for command in [
        "Get-Content x | Out-File Contract.docx",
        "Set-Content Contract.docx hi",
        "Add-Content Contract.docx hi",
        "cat x > Contract.docx",
        "Copy-Item x Contract.docx",
        "Move-Item Contract.docx y",
        "Remove-Item Contract.docx",
        "New-Item Contract.docx -Force",
        "& { cat Contract.docx }",
        "pwsh -Command \"cat Contract.docx\"",
    ] {
        assert_eq!(check(&fx, "PowerShell", command), contract, "{command}");
    }
}

/// stetdoc 0.9.0's forms, each both ways.
#[test]
fn stetdoc_reads_are_let_through_and_writes_to_a_held_file_refused() {
    let fx = Fixture::new("backstop-stetdoc");
    add(&fx, &["Contract.stet/x", "Other.stet/x"]);
    let contract = deny("Contract.docx");
    let allowed = [
        "stetdoc view Contract.docx --range 1-3",
        "stetdoc --json view Contract.docx",
        "stetdoc inspect Contract.docx 2.1",
        "stetdoc check Contract.docx --strict --author A",
        "stetdoc lookup Contract.docx indemnity",
        "stetdoc capabilities",
        "stetdoc skills --json",
        "stetdoc unpack Contract.docx",
        "stetdoc unpack Contract.docx -o work --refresh",
        "stetdoc edit Contract.docx --ops ops.txt",
        "stetdoc replace Contract.docx a b",
        "stetdoc comment Contract.docx 2.1 note",
        "stetdoc edit Contract.docx -o Out.docx",
        "stetdoc edit Free.docx --in-place",
        "stetdoc compare Contract.docx Free.docx -o Out.docx",
        "stetdoc pack Contract.stet -o Out.docx --force",
        "stetdoc pack Contract.stet",
        "stetdoc frobnicate Free.docx",
    ];
    for command in allowed {
        assert_eq!(check(&fx, "Bash", command), "", "{command}");
    }
    for command in [
        "stetdoc unpack Contract.docx -o Contract.docx",
        "stetdoc edit Contract.docx --in-place",
        "stetdoc edit --json Contract.docx --in-place",
        "stetdoc replace Contract.docx a b --in-place",
        "stetdoc edit Free.docx -o Contract.docx",
        "stetdoc comment Free.docx 1 x --output Contract.docx",
        "stetdoc compare Free.docx Other.docx -o Contract.docx",
        "stetdoc pack Contract.stet -o Contract.docx",
        "stetdoc frobnicate Contract.docx",
        "stetdoc view Contract.docx > Contract.docx",
    ] {
        assert_eq!(check(&fx, "Bash", command), contract, "{command}");
    }
    // Served or installed stetdoc is refused even naming nothing held.
    let serve = "Clauseo doesn't let the agent run stetdoc mcp or stetdoc init in its shell: \
                 run the stetdoc command you need directly.\n";
    for command in [
        "stetdoc mcp",
        "stetdoc --json init",
        "cd x && stetdoc mcp --stdio",
    ] {
        assert_eq!(check(&fx, "Bash", command), serve, "{command}");
    }
    // With the default output `Contract.edited.docx` held too: the forms writing it refuse.
    add(&fx, &["Contract.edited.docx", "~$ntract.edited.docx"]);
    let edited = deny("Contract.edited.docx");
    for command in [
        "stetdoc edit Contract.docx --ops ops.txt",
        "stetdoc comment Contract.docx 2.1 note",
        "stetdoc pack Contract.stet",
    ] {
        assert_eq!(check(&fx, "Bash", command), edited, "{command}");
    }
    for command in [
        "stetdoc edit Contract.docx --ops ops.txt --dry-run",
        "stetdoc pack Other.stet",
        "stetdoc edit Contract.docx -o Out.docx",
    ] {
        assert_eq!(check(&fx, "Bash", command), "", "{command}");
    }
}
