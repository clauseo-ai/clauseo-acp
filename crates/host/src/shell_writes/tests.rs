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
fn popd_leaves_the_folder_unknown() {
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
    ] {
        assert_eq!(targets_ps(script), none(), "{script}");
    }
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

/// The targets of a POSIX script, each as its path in the matter `/matters/Deal` (`None`:
/// outside it).
fn in_deal(script: &str) -> Vec<Option<String>> {
    let mut out = BTreeSet::new();
    add_script(&mut out, "", script, Dialect::Posix);
    out.iter()
        .map(|path| in_matter(Path::new("/matters/Deal"), path))
        .collect()
}

fn some(paths: &[&str]) -> Vec<Option<String>> {
    paths.iter().map(|path| Some((*path).to_owned())).collect()
}

/// Claude's memo flow: an assignment, a heredoc into a work folder, then a `cd` into the
/// matter before pandoc writes the reply there.
#[test]
fn a_cd_after_an_assignment_and_a_heredoc_sets_the_folder() {
    let script = "W=\"/data/run/abc/work\"\n\
        cat > \"$W/reply.md\" <<'EOF'\n# Reply\ncd /elsewhere && pandoc x -o y.docx\nEOF\n\
        cd /matters/Deal && pandoc \"$W/reply.md\" -o \"Reply to Kestrel legal notice - draft.docx\" \
        && rm \"$W/reply.md\" && python3 -I -c \"print(1)\"";
    assert_eq!(
        in_deal(script),
        some(&["Reply to Kestrel legal notice - draft.docx"])
    );
}

#[test]
fn a_cd_at_any_position_sets_the_folder_of_what_follows() {
    assert_eq!(
        in_deal("cd /matters/Deal && pandoc a.md -o rel.docx"),
        some(&["rel.docx"])
    );
    assert_eq!(
        in_deal("cd /matters/Deal/sub && pandoc a.md -o ../x.docx"),
        some(&["x.docx"])
    );
    assert_eq!(
        in_deal("echo x > /matters/Deal/a.md; cd /matters/Deal/Sub\npandoc a.md -o b.docx"),
        some(&["Sub/b.docx", "a.md"])
    );
    assert_eq!(
        targets("echo x > a.md && cd Sub && echo y > b.md"),
        ["Sub/b.md", "a.md"]
    );
    assert_eq!(
        targets("pushd Sub && pandoc in.md -o Memo.docx"),
        ["Sub/Memo.docx"]
    );
}

#[test]
fn a_powershell_set_location_at_any_position_sets_the_folder() {
    let folder = Path::new(r"C:\Matters\Deal");
    let in_ps = |script: &str| -> Vec<Option<String>> {
        let mut out = BTreeSet::new();
        add_script(&mut out, "", script, Dialect::PowerShell);
        out.iter().map(|path| in_matter(folder, path)).collect()
    };
    assert_eq!(
        in_ps(r"Set-Location 'C:\Matters\Deal'; stetdoc edit 'Main Agreement.docx' --ops o.json"),
        some(&["Main Agreement.edited.docx"])
    );
    assert_eq!(
        in_ps("$w = 'x'\nPush-Location 'C:\\Matters\\Deal'; pandoc a.md -o 'Re ply.docx'"),
        some(&["Re ply.docx"])
    );
    assert_eq!(
        targets_ps("stetdoc edit a.docx --ops o; Set-Location Sub; stetdoc edit b.docx --ops o"),
        ["Sub/b.edited.docx", "a.edited.docx"]
    );
}

/// A cd out of the matter, or one whose folder is unclear, counts no relative target after it;
/// absolute ones still count.
#[test]
fn a_cd_outside_or_unclear_counts_no_relative_target() {
    assert_eq!(
        in_deal("cd /elsewhere && pandoc a.md -o rel.docx"),
        vec![None]
    );
    assert_eq!(in_deal("cd .. && pandoc a.md -o rel.docx"), vec![]);
    for script in [
        r#"cd "$W" && pandoc a.md -o rel.docx"#,
        "cd x || exit 1; pandoc a.md -o y.docx",
        "ls | cd x && pandoc a.md -o y.docx",
        "cd x | pandoc a.md -o y.docx",
        "(cd x) && pandoc a.md -o y.docx",
        "{ cd x; } && pandoc a.md -o y.docx",
        "popd && pandoc a.md -o y.docx",
        "cd && pandoc a.md -o y.docx",
        "cd x & pandoc a.md -o y.docx",
    ] {
        assert_eq!(targets(script), none(), "{script}");
    }
    // a cd only part of an `&&` chain may not have run once the chain ends
    assert_eq!(targets("test -d x && cd x; pandoc a.md -o y.docx"), none());
    assert_eq!(
        in_deal(r#"cd "$W" && pandoc a.md -o /matters/Deal/abs.docx -o rel.docx"#),
        some(&["abs.docx"])
    );
    assert_eq!(targets_ps("Pop-Location; pandoc a.md -o y.docx"), none());
}
