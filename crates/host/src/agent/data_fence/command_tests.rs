//! The code owner's last review: a command's path words are expanded as the shell would (globs
//! and braces), the script given to `sh -c` and its kin is read one level down, and `grep -d
//! recurse` walks. Each is denied from the data folder's parent and passes in the matter folder
//! or a folder beside the data folder.

use serde_json::json;

use super::ShellKind;
use super::tests::{check, check_as, layout, text};

/// Requests that should be refused (`deny`) and ones that should pass (`pass`), each listed
/// when it does not.
fn judged(root: &std::path::Path, deny: &[String], pass: &[String]) {
    let judge = |line: &String| check(root, &[], Some(&json!({ "command": line }))).touches_data();
    let shown: Vec<&String> = deny.iter().filter(|line| !judge(line)).collect();
    let refused: Vec<&String> = pass.iter().filter(|line| judge(line)).collect();
    assert!(
        shown.is_empty() && refused.is_empty(),
        "shown: {shown:#?}\nrefused: {refused:#?}"
    );
}

/// A glob or a brace in a command's path word could match the data folder: it is read as a
/// pattern, from the folder before its first glob part. One in the matter folder or beside the
/// data folder passes.
#[test]
fn a_glob_or_brace_in_a_commands_path_word_is_read_as_a_pattern() {
    let dir = layout("cmd-glob");
    let root = dir.path();
    let parent = text(root);
    let matter = text(&root.join("matter"));
    let outside = text(&root.join("outside"));
    let deny = [
        format!("cat {parent}/dat*/settings.json"),
        format!("cat {parent}/[d]ata/settings.json"),
        format!("less {parent}/{{data,x}}/settings.json"),
        format!("head {parent}/d?ta/skill-packs/x/skills/brief/SKILL.md"),
        format!("cat {outside}/../da*/settings.json"),
    ];
    let pass = [
        format!("cat {matter}/sub/*.txt"),
        format!("ls {outside}/*"),
        format!("wc -l {matter}/{{sub,x}}/a.txt"),
        "cat sub/*.txt".to_owned(),
    ];
    judged(root, &deny, &pass);
}

/// `path` as a script given to a POSIX shell names it: with `/` on Windows (`D:/a/x`, as Git
/// Bash and agents write it there), since the shell reads a `\` in it as an escape.
fn in_script(path: &std::path::Path) -> String {
    let path = text(path);
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path
    }
}

/// The script `sh`, `bash`, `zsh` or `dash` is given with `-c` (or `-lc`, `-ec`), past
/// `sudo` and the like, is read as commands, one level down.
#[test]
fn a_script_given_to_a_shell_is_read_one_level_down() {
    let dir = layout("cmd-script");
    let root = dir.path();
    let parent = in_script(root);
    let data = in_script(&root.join("data"));
    let matter = in_script(&root.join("matter"));
    let outside = in_script(&root.join("outside"));
    let deny = [
        format!("bash -c 'grep -r x {parent}'"),
        format!(r#"sh -c "find {parent} -name SKILL.md""#),
        format!("bash -lc 'cd /tmp; ls -R {parent}'"),
        format!("zsh -ec 'du -sh {parent}'"),
        format!("dash -c 'cat {parent}/dat*/settings.json'"),
        format!("sudo bash -c 'tar czf /tmp/x.tgz {parent}'"),
        format!("echo hi | bash -c 'rg secret {parent}'"),
    ];
    let pass = [
        format!("bash -c 'grep -r x {matter}'"),
        format!("sh -lc 'ls {outside} && wc -l {matter}/sub/a.txt'"),
        format!("bash -c 'cat {data}/run/s1/work/out.txt'"),
    ];
    judged(root, &deny, &pass);
}

/// `grep -d recurse` and `--directories=recurse` walk the folders they are given.
#[test]
fn grep_told_to_recurse_by_directories_walks() {
    let dir = layout("cmd-grep-d");
    let root = dir.path();
    let parent = text(root);
    let matter = text(&root.join("matter"));
    let deny = [
        format!("grep -d recurse x {parent}"),
        format!("grep --directories=recurse x {parent}"),
        format!("grep -drecurse x {parent}"),
    ];
    let pass = [
        format!("grep -d skip x {parent}"),
        format!("grep -d recurse x {matter}"),
    ];
    judged(root, &deny, &pass);
}

/// PowerShell's short spellings of `-Recurse` walk as `-Recurse` does: `gci -r` from the data
/// folder's parent is denied, from the matter folder it passes, and Bash's `ls -r` (reverse
/// order) of the parent passes.
#[test]
fn powershell_short_recurse_spellings_walk() {
    let dir = layout("cmd-ps-recurse");
    let root = dir.path();
    let parent = text(root);
    let matter = text(&root.join("matter"));
    let deny = [
        format!("Get-ChildItem -Recurse {parent}"),
        format!("gci -r {parent}"),
        format!("Get-ChildItem -s {parent}"),
        format!("dir -Depth 5 {parent}"),
        format!("gci -Rec:$true {parent}"),
        format!("gci \u{2013}r {parent}"),
        format!("Remove-Item -r -Force {parent}"),
        format!(r#"pwsh -NoProfile -Command "ls -r '{parent}'""#),
    ];
    let pass = [
        format!("gci -r {matter}"),
        format!("ls -r {parent}"),
        format!("sort -r {matter}/sub/a.txt"),
        format!("gci -d {parent}"),
    ];
    judged(root, &deny, &pass);
}

/// The tool that sent the line decides how `ls -r` and `dir -r` are read: from PowerShell's
/// tool `ls -r` of the data folder's parent walks it and is denied; from Bash's, `ls -r` and
/// GNU's `dir -r` only sort in reverse and pass, while `dir -R` walks and is denied.
#[test]
fn the_tools_shell_decides_whether_ls_r_walks() {
    let dir = layout("cmd-tool-shell");
    let root = dir.path();
    let parent = text(root);
    let matter = text(&root.join("matter"));
    let judge = |shell, line: String| {
        let input = json!({ "command": line });
        check_as(root, &[], Some(&input), shell).touches_data()
    };
    for line in [
        format!("ls -r {parent}"),
        format!("dir -r {parent}"),
        format!("rm -s {parent}"),
        format!("cp -Rec {parent} {matter}"),
    ] {
        assert!(
            judge(ShellKind::PowerShell, line.clone()),
            "PowerShell passed: {line}"
        );
    }
    assert!(!judge(ShellKind::PowerShell, format!("ls -r {matter}")));
    for line in [format!("ls -r {parent}"), format!("dir -r {parent}")] {
        assert!(
            !judge(ShellKind::Posix, line.clone()),
            "Bash denied: {line}"
        );
    }
    assert!(judge(ShellKind::Posix, format!("dir -R {parent}")));
    assert!(judge(ShellKind::Posix, format!("ls -R {parent}")));
}
