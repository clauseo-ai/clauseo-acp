//! The stetdoc read fence end to end: Claude's entry passes the fence, and the guard refuses a
//! read of another matter's document with words #1085's refusal detector takes for a refusal,
//! without naming that matter's folder.

use serde_json::json;

use super::{Fixture, guard_with};

/// What the guard says of a line that is not the plain shape, and of a word it can't follow.
const PLAIN: &str = "stetdoc runs only as a plain command";

/// The fixture's `out` folder as another matter: a document there, read through a link in the
/// matter, is refused, saying "was denied" but not where that matter is; a document beside it
/// that no fence names (a template in Documents) reads as before.
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
        assert!(!said.contains(&other), "{tool}: {said}");
    }
    let template = format!("stetdoc view {}", fx.path("docs/template.docx"));
    let hook = fx.hook("Bash", &json!({"command": template}));
    assert_eq!(guard_with(&args, hook.to_string().as_bytes()), "");
}

/// A document with a second name (a hard link to another matter's document) is refused; the
/// matter's own document, with one name, reads.
#[cfg(unix)]
#[test]
fn a_document_with_a_second_name_is_refused() {
    let fx = Fixture::new("stetdoc-hard-link");
    crate::write::hard_link(
        &fx.root.join("out/Held.docx"),
        &fx.matter().join("Copy.docx"),
    )
    .unwrap();
    let (matter, other) = (fx.path("m"), fx.path("out"));
    let args = ["--matter", &matter, "--fence-matter", &other];
    let hook = fx.hook("Bash", &json!({"command": "stetdoc view Copy.docx"}));
    let said = guard_with(&args, hook.to_string().as_bytes());
    assert!(
        said.contains("couldn't tell where Copy.docx leads"),
        "{said}"
    );
    let hook = fx.hook(
        "Bash",
        &json!({"command": "stetdoc view Free.docx | head -5"}),
    );
    assert_eq!(guard_with(&args, hook.to_string().as_bytes()), "");
}

/// A stetdoc line that is not the plain shape is refused, the refusal saying what the plain
/// shape is; a line that never mentions stetdoc is not this fence's.
#[test]
fn a_line_not_of_the_plain_shape_is_refused_with_the_plain_form() {
    let fx = Fixture::new("stetdoc-shape");
    let (matter, other) = (fx.path("m"), fx.path("out"));
    let args = ["--matter", &matter, "--fence-matter", &other];
    for (tool, command) in [
        ("Bash", "env stetdoc view Contract.docx"),
        ("Bash", "cd sub; stetdoc view Contract.docx"),
        ("PowerShell", "stetdoc view (Get-Item Contract.docx)"),
    ] {
        let hook = fx.hook(tool, &json!({"command": command}));
        let said = guard_with(&args, hook.to_string().as_bytes());
        assert!(said.contains(PLAIN), "{tool} {command}: {said}");
    }
    let hook = fx.hook("Bash", &json!({"command": "ls *.docx | head"}));
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

/// On the Mac, the data folder written on the data volume (`/System/Volumes/Data/private/…`)
/// and as the firmlink shows it (`/private/…`), in the fence or in the read, either way round:
/// refused as the data folder's. The temp folder is under `/private`, a firmlink as `/Users`
/// is; skipped where it isn't one.
#[cfg(target_os = "macos")]
#[test]
fn the_data_folders_data_volume_path_is_refused() {
    let fx = Fixture::new("stetdoc-firmlink");
    crate::write::create_dir_all(&fx.root.join("data").join("logs")).unwrap();
    crate::write::write_file(&fx.root.join("data/logs/host.docx"), b"x").unwrap();
    let real = dunce::canonicalize(fx.root.join("data")).unwrap();
    let on_volume =
        std::path::Path::new("/System/Volumes/Data").join(real.strip_prefix("/").unwrap());
    let net = crate::network_paths::NetEnv::current();
    if !on_volume.exists() || net.root_form(&on_volume) != real {
        eprintln!(
            "skipped: {} is not a firmlinked spelling here",
            on_volume.display()
        );
        return;
    }
    let (real, on_volume) = (real.to_str().unwrap(), on_volume.to_str().unwrap());
    let matter = fx.path("m");
    for (fenced, read) in [(real, on_volume), (on_volume, real)] {
        let args = ["--matter", &matter, "--fence-data", fenced];
        let command = format!("stetdoc view '{read}/logs/host.docx'");
        let hook = fx.hook("Bash", &json!({"command": command}));
        let got = guard_with(&args, hook.to_string().as_bytes());
        assert!(
            got.contains("it is in Clauseo's own data folder"),
            "{fenced} {command}: {got}"
        );
    }
}
