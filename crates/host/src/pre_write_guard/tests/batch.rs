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
    let dotdot = format!("cp Revised/*.docx {}/../m", fx.outside());
    let copy_out = format!("cp *.docx {}", fx.outside());
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
            &copy_out,
            "cp Revised/[!C]*.docx .",
            "cat *.docx | head",
            "ls -l *.docx",
            "echo hi > notes.txt",
        ],
    );
    #[cfg(unix)]
    allowed(&fx, "Bash", &["cp *.docx /tmp"]);
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
    let unzip_out = format!("unzip -o Contract.docx -d {}", fx.outside());
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
            &unzip_out,
        ],
    );
    #[cfg(unix)]
    allowed(&fx, "Bash", &["unzip -q Contract.docx -d/tmp"]);
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

/// Review round 5: `unzip -d` reads only when the folder is outside the matter; into it, a
/// member can land on the held file.
#[test]
fn unzip_into_the_matter_is_refused() {
    let fx = Fixture::new("batch-unzip");
    let into_matter = format!("unzip -o Revised.zip Contract.docx -d {}", fx.path("m"));
    refused(
        &fx,
        "Bash",
        &[
            &into_matter,
            "unzip -o Revised.zip Contract.docx -d ../m",
            "unzip -o Revised.zip Contract.docx -d sub/..",
            "unzip -o Contract.docx -d unpacked",
        ],
    );
    let out = format!("unzip -o Revised.zip Contract.docx -d {}", fx.outside());
    allowed(&fx, "Bash", &[&out]);
    #[cfg(unix)]
    allowed(&fx, "Bash", &["unzip -o Contract.docx -d /tmp"]);
}

/// On Windows a folder outside the matter is recognised only as a drive path (`C:/…`, or
/// `C:\…` in quotes): `/tmp` or Git Bash's `/c/…` is no absolute path there, so a copy or
/// extraction into it is judged like one into the matter (refused, the safe side).
#[cfg(windows)]
#[test]
fn on_windows_an_outside_folder_is_a_drive_path() {
    let fx = Fixture::new("batch-windows");
    let forward = fx.outside();
    let quoted = format!("unzip -o Contract.docx -d '{}'", fx.path("out"));
    let copy = format!("cp *.docx {forward}");
    let extract = format!("unzip -o Contract.docx -d {forward}");
    allowed(&fx, "Bash", &[&copy, &extract, &quoted]);
    let git_bash = format!(
        "cp *.docx /{}",
        forward.replacen(':', "", 1).to_ascii_lowercase()
    );
    refused(
        &fx,
        "Bash",
        &[
            "cp *.docx /tmp",
            "unzip -o Contract.docx -d /tmp",
            &git_bash,
        ],
    );
}

/// Review round 5: a quoted pattern a search program matches itself is no file operand, so it
/// does not count as naming the held file; unquoted, or with an action that writes, it still does.
#[test]
fn a_quoted_search_pattern_is_not_a_wildcard_operand() {
    let fx = Fixture::new("batch-patterns");
    allowed(
        &fx,
        "Bash",
        &[
            "find . -name '*.docx'",
            "find . -iname \"*.DOCX\" | wc -l",
            "rg --files -g '*.docx'",
            "rg --files --glob='*.docx' | sort",
            "grep -rl X --include='*.docx' .",
        ],
    );
    allowed(&fx, "PowerShell", &["Get-ChildItem -Filter '*.docx'"]);
    refused(
        &fx,
        "Bash",
        &[
            "find . -name *.docx -delete",
            "find . -name '*.docx' -delete",
            "find . -name '*.docx' -exec rm {} +",
            "find . -name '*.docx' | xargs rm",
            "rg --files -g '*.docx' | xargs rm",
        ],
    );
    refused(
        &fx,
        "PowerShell",
        &["Get-ChildItem -Filter '*.docx' | Remove-Item"],
    );
}
