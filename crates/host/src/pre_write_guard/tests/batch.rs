//! Review round 4: a path-layer hit inside an option word, batch commands over a wildcard,
//! textutil's default output, and harmless reads the exemption takes back.

use super::backstop::check;
use super::{Fixture, deny};

/// Commands with `tool` that must be refused naming `Contract.docx`, each checked; the ones let
/// through are listed together.
fn refused(fx: &Fixture, tool: &str, commands: &[&str]) {
    let contract = deny("Contract.docx");
    let let_through: Vec<&&str> = commands
        .iter()
        .filter(|command| check(fx, tool, command) != contract)
        .collect();
    assert!(
        let_through.is_empty(),
        "{tool} let through: {let_through:#?}"
    );
}

/// Commands with `tool` that must go on.
fn allowed(fx: &Fixture, tool: &str, commands: &[&str]) {
    let stopped: Vec<(&&str, String)> = commands
        .iter()
        .map(|command| (command, check(fx, tool, command)))
        .filter(|(_, said)| !said.is_empty())
        .collect();
    assert!(stopped.is_empty(), "{tool} refused: {stopped:#?}");
}

/// Finding 1 (a round-1 regression): the path layer remembers the whole word
/// `--output=Link.docx`; the output's value `Link.docx` is a part of it, so it is held too.
#[cfg(unix)]
#[test]
fn an_output_value_through_a_link_inside_an_option_word_is_refused() {
    let fx = Fixture::new("batch-link");
    crate::write::symlink(
        &fx.matter().join("Contract.docx"),
        &fx.matter().join("Link.docx"),
    )
    .unwrap();
    refused(
        &fx,
        "Bash",
        &[
            "pandoc notes.md --output=Link.docx",
            "pandoc notes.md --out=Link.docx",
            "stetdoc edit Free.docx --ops ops.json --output=Link.docx",
            "stetdoc edit Free.docx --ops ops.json -o=Link.docx",
        ],
    );
    allowed(&fx, "Bash", &["pandoc Link.docx --output=notes.md"]);
}

/// Finding 2: a wildcard that could match a held file's name counts as naming it, so a batch
/// command over it is judged like one naming the file.
#[test]
fn a_batch_command_over_a_wildcard_matching_a_held_name_is_refused() {
    let fx = Fixture::new("batch-glob");
    refused(
        &fx,
        "Bash",
        &[
            "cp Revised/*.docx .",
            "cp Revised/* .",
            "cp Revised/Con?ract.docx .",
            "cp Revised/[A-D]*.docx .",
            "for f in *.docx; do stetdoc replace \"$f\" \"Old Co\" \"New Co\" --in-place; done",
            "rm *.docx",
            "find . -name '*.docx' -delete",
            "textutil -convert docx *.txt",
        ],
    );
    refused(&fx, "PowerShell", &[r"Copy-Item Revised\* ."]);
    let into_matter = format!("cp Revised/*.docx {}", fx.path("m"));
    let dotdot = format!("cp Revised/*.docx {}/../m", fx.path("out"));
    refused(
        &fx,
        "Bash",
        &[&into_matter, &dotdot, "cp Revised/*.docx Archive/"],
    );
    allowed(
        &fx,
        "Bash",
        &[
            "cp Revised/*.txt .",
            "cp *.docx /tmp",
            "cp Revised/[!C]*.docx .",
            "cat *.docx | head",
            "ls -l *.docx",
            "echo hi > notes.txt",
        ],
    );
}

/// Finding 3: textutil writes its input's name with the new extension beside it, held or not
/// named in the command; `-extension` and `-output` change it.
#[test]
fn textutil_writing_a_held_file_by_its_default_name_is_refused() {
    let fx = Fixture::new("batch-textutil");
    refused(
        &fx,
        "Bash",
        &[
            "textutil -convert docx Contract.txt",
            "textutil -convert docx Contract.rtf",
            "cd sub && textutil -convert docx ../Contract.rtf",
            "textutil -convert html -extension docx Contract.txt",
            "textutil -convert docx Notes.txt -output Contract.docx",
            "textutil -cat docx a.txt b.txt -output Contract.docx",
            "textutil -convert docx -frobnicate Contract.docx",
        ],
    );
    allowed(
        &fx,
        "Bash",
        &[
            "textutil -convert txt Contract.docx",
            "textutil -convert txt Contract.docx -output other.txt",
            "textutil -convert docx -extension rtfd Contract.txt",
            "textutil -convert docx Notes.txt",
            "textutil -info Contract.docx",
            "textutil -convert txt -stdout Contract.docx | head",
        ],
    );
}

/// Item 4 of the round-4 decisions: reads and copies away from the held file go on; the same
/// programs writing it are refused.
#[test]
fn harmless_reads_and_copies_away_go_on() {
    let fx = Fixture::new("batch-reads");
    allowed(
        &fx,
        "Bash",
        &[
            "cp Contract.docx other.docx",
            "cp -p Contract.docx Backup/Contract-copy.docx",
            "ls -la",
            "ls -l Contract.docx",
            "stat Contract.docx",
            "test -f Contract.docx && echo yes",
            "[ -f Contract.docx ] && echo yes",
            "shasum Contract.docx",
            "md5sum Contract.docx",
            "sha256sum Contract.docx",
            "zipinfo Contract.docx",
            "diff Contract.docx Free.docx",
            "cmp Contract.docx Free.docx",
            "unzip -o Contract.docx -d unpacked",
            "unzip -q Contract.docx -d/tmp/contract-parts",
        ],
    );
    allowed(
        &fx,
        "PowerShell",
        &[
            "Get-Content Contract.docx",
            "type Contract.docx",
            "Select-String -Path Contract.docx -Pattern x",
            "Test-Path Contract.docx",
        ],
    );
    refused(
        &fx,
        "Bash",
        &[
            "cp Free.docx Contract.docx",
            "cp Revised/Contract.docx .",
            "cp Revised/Contract.docx ./",
            "cp Contract.docx backup",
            "cp -r Contract.docx x.docx",
            "cp a.docx Contract.docx b/",
            "unzip -o Contract.docx",
            "unzip -o Contract.docx -d .",
            "unzip Contract.docx -x a -d out",
            "ls > Contract.docx",
            "stat Contract.docx > Contract.docx",
            "md5sum x > Contract.docx",
        ],
    );
    refused(
        &fx,
        "PowerShell",
        &[
            "Get-Content x | Set-Content Contract.docx",
            "type x > Contract.docx",
        ],
    );
}
