use std::collections::BTreeSet;
use std::path::Path;

use super::{add_input, add_script, in_matter};

fn targets(script: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    add_script(&mut out, "", script);
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
    );
    add_input(
        &mut out,
        r#"{"command":["bash","-lc","echo x > b.md"],"cwd":"/m/Sub"}"#,
    );
    add_input(&mut out, r#"{"command":["cp","a.docx","c.docx"]}"#);
    add_input(&mut out, "not json > d.md");
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
