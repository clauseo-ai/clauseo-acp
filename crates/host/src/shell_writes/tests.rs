use std::collections::BTreeSet;
use std::path::Path;

use super::{Dialect, add_input, add_script, in_matter};

fn targets(script: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    add_script(&mut out, "", script, Dialect::Posix);
    out.into_iter().collect()
}

fn none() -> Vec<String> {
    Vec::new()
}

#[test]
fn reading_listing_and_searching_write_nothing() {
    assert_eq!(targets("ls -la"), none());
    assert_eq!(targets("find . -name '*.docx'"), none());
    assert_eq!(targets(r#"grep "Memo.docx" notes.txt"#), none());
    assert_eq!(targets("grep -o Memo.docx notes.txt"), none());
    assert_eq!(targets("cat Memo.docx"), none());
    assert_eq!(targets("pandoc in.md < Memo.docx"), none());
}

#[test]
fn write_targets_count() {
    assert_eq!(targets(r#"pandoc in.md -o "Reply.docx""#), ["Reply.docx"]);
    assert_eq!(targets("echo x > notes.md"), ["notes.md"]);
    assert_eq!(targets("echo x>>log.txt 2>&1"), ["log.txt"]);
    assert_eq!(targets("make 2> err.txt"), ["err.txt"]);
    assert_eq!(targets("cp a.docx Sub/Memo.docx"), ["Sub/Memo.docx"]);
    assert_eq!(targets("cp -f draft.docx out/Memo.docx"), ["out/Memo.docx"]);
    assert_eq!(
        targets("pandoc a.md | tee one.md two.md"),
        ["one.md", "two.md"]
    );
    assert_eq!(
        targets("pandoc a.md --output=./out/../Reply.docx"),
        ["Reply.docx"]
    );
    assert_eq!(
        targets(r#"pandoc in.md -o 'Reply to notice.docx' && cp x "Sub/b c.txt""#),
        ["Reply to notice.docx", "Sub/b c.txt"]
    );
}

#[test]
fn a_leading_cd_sets_the_folder_and_anything_unclear_writes_nothing() {
    assert_eq!(
        targets("cd Sub && pandoc a.md -o Memo.docx"),
        ["Sub/Memo.docx"]
    );
    assert_eq!(targets("cd Sub; cd ../Out && echo x > a.md"), ["Out/a.md"]);
    // a cd after another command: the folder is no longer known
    assert_eq!(
        targets("echo x > a.md && cd Sub && echo y > b.md"),
        ["a.md"]
    );
    assert_eq!(targets("echo x > $OUT"), none());
    assert_eq!(targets(r#"echo x > "$HOME/a.md""#), none());
    assert_eq!(targets("cp a.docx *.bak"), none());
    assert_eq!(targets("cp a.docx Sub/"), none());
    assert_eq!(targets("echo x > ../outside.md"), none());
    assert_eq!(targets("cp a.docx"), none());
}

#[test]
fn a_heredoc_body_is_text_not_commands() {
    assert_eq!(
        targets("cat > notes.md <<'EOF'\npandoc in.md -o Memo.docx\nEOF"),
        ["notes.md"]
    );
    // `<<-` strips leading tabs from the delimiter line, and a bare or double-quoted
    // delimiter works too
    assert_eq!(
        targets("cat > a.md <<-\"END\"\n\tcp x Memo.docx\n\tEND\necho y > b.md"),
        ["a.md", "b.md"]
    );
    assert_eq!(
        targets("cat <<EOF > a.md\necho > Memo.docx\nEOF\ncp x c.docx"),
        ["a.md", "c.docx"]
    );
}

#[test]
fn pushd_and_popd_leave_the_folder_unknown() {
    assert_eq!(targets("pushd Sub && pandoc in.md -o Memo.docx"), none());
    assert_eq!(targets("echo x > a.md && popd && echo y > b.md"), ["a.md"]);
}

#[test]
fn mv_writes_nothing() {
    assert_eq!(targets("mv lawyer.docx tmp && mv tmp lawyer.docx"), none());
    assert_eq!(targets("mv -t Sub lawyer.docx"), none());
}

#[test]
fn unzip_dash_o_writes_nothing() {
    assert_eq!(targets("unzip -o pack.zip"), none());
}

#[test]
fn input_gives_the_command_and_its_folder() {
    let mut out = BTreeSet::new();
    add_input(
        &mut out,
        r#"{"command":"pandoc in.md -o \"Reply.docx\"","description":"x"}"#,
        Dialect::Posix,
    );
    add_input(
        &mut out,
        r#"{"command":["bash","-lc","echo x > b.md"],"cwd":"/m/Sub"}"#,
        Dialect::Posix,
    );
    add_input(
        &mut out,
        r#"{"command":["cp","a.docx","c.docx"]}"#,
        Dialect::Posix,
    );
    add_input(&mut out, "not json > d.md", Dialect::Posix);
    let out: Vec<&str> = out.iter().map(String::as_str).collect();
    assert_eq!(out, ["/m/Sub/b.md", "Reply.docx", "c.docx"]);
    assert_eq!(
        in_matter(Path::new("/m"), "/m/Sub/b.md").as_deref(),
        Some("Sub/b.md")
    );
    assert_eq!(in_matter(Path::new("/m"), "/elsewhere/b.md"), None);
    assert_eq!(in_matter(Path::new("/m"), "/mm/b.md"), None);
    assert_eq!(
        in_matter(Path::new("/m"), "Reply.docx").as_deref(),
        Some("Reply.docx")
    );
}

fn targets_ps(script: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    add_script(&mut out, "", script, Dialect::PowerShell);
    out.into_iter().collect()
}

/// stetdoc's writing forms in Bash (`crate::stetdoc_args`): the default output beside the
/// document, `--in-place` the document, an output option its value; `--dry-run`, a reading
/// subcommand and anything clap would refuse write nothing.
#[test]
fn stetdoc_writes_its_default_output_the_document_in_place_or_its_output() {
    let folder = Path::new("/matters/Deal");
    let mut out = BTreeSet::new();
    add_script(
        &mut out,
        "",
        r#"cd "/matters/Deal" && stetdoc edit "Main Agreement.docx" --ops ops.json"#,
        Dialect::Posix,
    );
    let out: Vec<Option<String>> = out.iter().map(|path| in_matter(folder, path)).collect();
    assert_eq!(out, [Some("Main Agreement.edited.docx".to_owned())]);
    for (script, written) in [
        (
            r#"stetdoc replace "Main Agreement.docx" "30 days" "45 days" --in-place"#,
            "Main Agreement.docx",
        ),
        (
            r#"stetdoc --json comment "Main Agreement.docx" Term "Check this" -o "Reply.docx""#,
            "Reply.docx",
        ),
        (
            "stetdoc replace 'Main Agreement.docx' a b --all --author Jane",
            "Main Agreement.edited.docx",
        ),
        (
            r"stetdoc edit Drafts/Main\ Agreement.docx --ops ops.json",
            "Drafts/Main Agreement.edited.docx",
        ),
        (
            r#"stetdoc pack "Main Agreement.stet/" --done"#,
            "Main Agreement.edited.docx",
        ),
        (
            "stetdoc edit a.docx --ops o.json --output=Out.docx",
            "Out.docx",
        ),
        ("stetdoc edit a.docx --ops o.json -oOut.docx", "Out.docx"),
        ("stetdoc compare a.docx b.docx -o Diff.docx", "Diff.docx"),
        ("./bin/stetdoc edit a.docx --ops o.json", "a.edited.docx"),
    ] {
        assert_eq!(targets(script), [written], "{script}");
    }
    for script in [
        r#"stetdoc edit "Main Agreement.docx" --ops ops.json --dry-run"#,
        r#"stetdoc replace "Main Agreement.docx" a b -o "Reply.docx" --dry-run"#,
        r#"stetdoc view "Main Agreement.docx""#,
        r#"stetdoc --json inspect "Main Agreement.docx" -o x.docx"#,
        "stetdoc unpack a.docx",
        "stetdoc edit a.docx --frobnicate",
        "stetdoc edit a.docx -i",
        r#"stetdoc edit "$DOC" --ops o.json"#,
        "stetdoc edit a.docx --ops o.json -o -",
        "stetdoc help edit",
        "stetdoc",
    ] {
        assert_eq!(targets(script), none(), "{script}");
    }
}

/// The same forms as Claude Code's PowerShell tool writes them: quotes as PowerShell reads
/// them, `\` a folder separator, `cd`/`Set-Location` setting the folder as Bash's `cd` does.
#[test]
fn stetdoc_in_powershell() {
    let folder = Path::new(r"C:\Matters\Deal");
    let in_deal = |script: &str| -> Vec<Option<String>> {
        let mut out = BTreeSet::new();
        add_script(&mut out, "", script, Dialect::PowerShell);
        out.iter().map(|path| in_matter(folder, path)).collect()
    };
    let some = |path: &str| vec![Some(path.to_owned())];
    assert_eq!(
        in_deal(r#"cd "C:\Matters\Deal"; stetdoc edit "Main Agreement.docx" --ops ops.json"#),
        some("Main Agreement.edited.docx")
    );
    assert_eq!(
        in_deal(
            r"Set-Location -Path 'C:\Matters\Deal'; stetdoc.exe replace 'Main Agreement.docx' '30 days' '45 days' --in-place"
        ),
        some("Main Agreement.docx")
    );
    assert_eq!(
        in_deal(r#"sl C:\Matters\Deal\Drafts && stetdoc edit "Main Agreement.docx" --ops o.json"#),
        some("Drafts/Main Agreement.edited.docx")
    );
    for (script, written) in [
        (
            r#"stetdoc comment 'Main Agreement.docx' Term 'It''s long' -o "Reply.docx""#,
            "Reply.docx",
        ),
        (
            r#"stetdoc edit "Drafts\Main Agreement.docx" --ops ops.json"#,
            "Drafts/Main Agreement.edited.docx",
        ),
        (
            r#"& stetdoc edit "A ""quoted"" name.docx" --ops o.json"#,
            r#"A "quoted" name.edited.docx"#,
        ),
        (
            "# make the reply\nstetdoc pack 'Main Agreement.stet\\'",
            "Main Agreement.edited.docx",
        ),
        ("stetdoc view a.docx > out.txt 2>&1", "out.txt"),
    ] {
        assert_eq!(targets_ps(script), [written], "{script}");
    }
    for script in [
        r#"stetdoc edit "Main Agreement.docx" --ops ops.json --dry-run"#,
        r#"stetdoc view "Main Agreement.docx""#,
        "stetdoc edit $doc --ops o.json",
        r#"stetdoc edit "$env:TEMP\a.docx" --ops o.json"#,
        r#"stetdoc edit `"a.docx" --ops o.json"#,
        "stetdoc edit @args",
        "stetdoc edit *.docx --ops o.json",
        "Push-Location Sub; stetdoc edit a.docx --ops o.json",
    ] {
        assert_eq!(targets_ps(script), none(), "{script}");
    }
    // a later folder change leaves the folder unknown
    assert_eq!(
        targets_ps("stetdoc edit a.docx --ops o; Set-Location Sub; stetdoc edit b.docx --ops o"),
        ["a.edited.docx"]
    );
}

/// A PowerShell call's input is read as PowerShell; any other tool's as POSIX.
#[test]
fn a_powershell_input_is_read_as_powershell() {
    assert_eq!(Dialect::of_tool(Some("PowerShell")), Dialect::PowerShell);
    assert_eq!(Dialect::of_tool(Some("Bash")), Dialect::Posix);
    assert_eq!(Dialect::of_tool(None), Dialect::Posix);
    let input = r#"{"command":"cd \"C:\\Matters\\Deal\"; stetdoc edit \"Main Agreement.docx\" --ops ops.json","description":"x"}"#;
    let mut out = BTreeSet::new();
    add_input(&mut out, input, Dialect::PowerShell);
    let out: Vec<&str> = out.iter().map(String::as_str).collect();
    assert_eq!(out, ["C:/Matters/Deal/Main Agreement.edited.docx"]);
}
