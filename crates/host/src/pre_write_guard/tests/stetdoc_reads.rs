//! The stetdoc read fence end to end: Claude's entry passes the fence, and the guard refuses a
//! read of another matter's document with words #1085's refusal detector takes for a refusal.

use serde_json::json;

use super::{Fixture, guard_with};

/// The fixture's `out` folder as another matter: a document there, read through a link in the
/// matter, is refused, saying "was denied" and naming that matter's folder; a document beside
/// it that no fence names (a template in Documents) reads as before.
#[cfg(unix)]
#[test]
fn another_matter_is_refused_and_a_template_reads() {
    let fx = Fixture::new("stetdoc-reads");
    let other = fx.path("out");
    crate::write::create_dir_all(&fx.root.join("docs")).unwrap();
    crate::write::write_file(&fx.root.join("docs").join("template.docx"), b"x").unwrap();
    crate::write::symlink(&fx.root.join("out"), &fx.matter().join("link")).unwrap();
    let matter = fx.path("m");
    let args = ["--matter", &matter, "--fence-matter", &other];
    for (tool, command) in [
        ("Bash", "stetdoc view link/Held.docx"),
        ("PowerShell", r"stetdoc view link\Held.docx"),
    ] {
        let hook = fx.hook(tool, &json!({"command": command}));
        let said = guard_with(&args, hook.to_string().as_bytes());
        assert!(said.contains("was denied"), "{tool}: {said}");
        assert!(said.contains(&format!("({other})")), "{tool}: {said}");
    }
    let template = format!("stetdoc view {}", fx.path("docs/template.docx"));
    let hook = fx.hook("Bash", &json!({"command": template}));
    assert_eq!(guard_with(&args, hook.to_string().as_bytes()), "");
}

/// Without a fence (Codex's hook, or a session before this) stetdoc reads anywhere as before.
#[test]
fn no_fence_no_refusal() {
    let fx = Fixture::new("stetdoc-no-fence");
    let held = fx.path("out/Held.docx").replace('\\', "/");
    let hook = fx.hook("Bash", &json!({"command": format!("stetdoc view {held}")}));
    let matter = fx.path("m");
    assert_eq!(
        guard_with(&["--matter", &matter], hook.to_string().as_bytes()),
        ""
    );
}
