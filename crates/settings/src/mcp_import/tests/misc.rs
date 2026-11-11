//! Variable expansion, path rules and the real filesystem.

use super::*;

#[test]
fn expansion_follows_bash_style_defaults() {
    let vars = vars(&[("SET", "value"), ("EMPTY", "")]);
    let expand_text = |text: &str| {
        let mut problems = Vec::new();
        let out = expand::expand(text, &vars, &mut problems);
        (out, problems)
    };
    assert_eq!(expand_text("a ${SET} b").0, "a value b");
    assert_eq!(expand_text("${UNSET:-d}").0, "d");
    assert_eq!(expand_text("${EMPTY:-d}").0, "d");
    assert_eq!(expand_text("${SET:-d}").0, "value");
    assert_eq!(expand_text("${UNSET:-}").0, "");
    assert_eq!(expand_text("${SET}${SET}").0, "valuevalue");
    assert_eq!(expand_text("$SET and ${").0, "$SET and ${");
    assert_eq!(expand_text("${1BAD} ${a-b} ${}").0, "${1BAD} ${a-b} ${}");
    assert_eq!(expand_text("${open").0, "${open");
    let (out, problems) = expand_text("x${NOPE}y${NOPE}");
    assert_eq!(out, "x${NOPE}y${NOPE}");
    let nope = Problem::UnresolvedVariable {
        name: "NOPE".to_owned(),
    };
    assert_eq!(problems, [nope.clone(), nope]);
}

#[test]
fn paths_join_and_split_by_os() {
    assert_eq!(
        join(Os::Mac, "/Users/ana/", ".claude.json"),
        "/Users/ana/.claude.json"
    );
    assert_eq!(
        join(Os::Windows, "C:\\Users\\ana\\", "Claude/config.json"),
        "C:\\Users\\ana\\Claude\\config.json"
    );
    assert!(is_absolute(Os::Mac, "/bin/x"));
    assert!(!is_absolute(Os::Mac, "C:\\x"));
    assert!(is_absolute(Os::Windows, "C:\\x\\y.exe"));
    assert!(is_absolute(Os::Windows, "C:/x/y.exe"));
    assert!(is_absolute(Os::Windows, "\\\\server\\share\\x"));
    assert!(!is_absolute(Os::Windows, "\\rooted"));
    assert!(!is_absolute(Os::Windows, "npx"));
    assert_eq!(file_name(Os::Windows, "C:\\a\\b.cmd"), "b.cmd");
    assert_eq!(file_name(Os::Mac, "/a/b"), "b");
}

#[test]
fn real_fs_reads_this_crate() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fs = RealFs;
    assert!(fs.is_file(&manifest.join("Cargo.toml")));
    assert!(!fs.is_file(&manifest.join("no-such-file.json")));
    assert!(fs.read_to_string(&manifest.join("Cargo.toml")).is_some());
    assert!(
        fs.read_to_string(&manifest.join("no-such-file.json"))
            .is_none()
    );
    let src = fs.list_dir(&manifest.join("src"));
    assert!(src.iter().any(|entry| entry.ends_with("lib.rs")));
    assert_eq!(fs.list_dir(&manifest.join("no-such-dir")).len(), 0);
}
