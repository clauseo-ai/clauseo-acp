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

/// The words the guard says for a read of the data folder.
const DATA_SAID: &str = "it is in Clauseo's own data folder";

/// A real directory junction in the matter into another matter and into the data folder, each
/// read in both shells: refused, naming the other matter, or as the data folder's.
#[cfg(windows)]
#[test]
fn a_junction_into_another_matter_or_the_data_folder_is_refused() {
    let fx = Fixture::new("stetdoc-junction");
    let (other, data) = (fx.path("out"), fx.path("data"));
    crate::write::create_dir_all(&fx.root.join("data").join("logs")).unwrap();
    crate::write::write_file(&fx.root.join(r"data\logs\host.docx"), b"x").unwrap();
    let junction = clauseo_sandbox::windows::test_junction::junction;
    junction(&fx.matter().join("toB"), &fx.root.join("out")).unwrap();
    junction(&fx.matter().join("toData"), &fx.root.join("data")).unwrap();
    let matter = fx.path("m");
    let args = [
        "--matter",
        &matter,
        "--fence-matter",
        &other,
        "--fence-data",
        &data,
    ];
    for (tool, command, said) in [
        ("Bash", "stetdoc view toB/Held.docx", format!("({other})")),
        (
            "PowerShell",
            r"stetdoc view toB\Held.docx",
            format!("({other})"),
        ),
        (
            "Bash",
            "stetdoc view toData/logs/host.docx",
            DATA_SAID.to_owned(),
        ),
        (
            "PowerShell",
            r"stetdoc view toData\logs\host.docx",
            DATA_SAID.to_owned(),
        ),
    ] {
        let hook = fx.hook(tool, &json!({"command": command}));
        let got = guard_with(&args, hook.to_string().as_bytes());
        assert!(got.contains(&said), "{tool} {command}: {got}");
    }
}

/// The 8.3 short name of another matter's long-named folder is that folder: refused, naming
/// it. Skipped where the volume makes no short names.
#[cfg(windows)]
#[test]
fn the_short_name_of_another_matters_folder_is_refused() {
    let fx = Fixture::new("stetdoc-short-name");
    let long = fx.root.join("Another Matter Folder");
    crate::write::create_dir_all(&long).unwrap();
    crate::write::write_file(&long.join("Held.docx"), b"x").unwrap();
    let Some(short) = crate::network_paths::short_path(&long).filter(|short| *short != long) else {
        eprintln!("skipped: the volume gave no 8.3 short name for the test folder");
        return;
    };
    let other = long.to_str().unwrap().to_owned();
    let read = short.join("Held.docx").to_str().unwrap().to_owned();
    let matter = fx.path("m");
    let args = ["--matter", &matter, "--fence-matter", &other];
    for (tool, command) in [
        (
            "Bash",
            format!("stetdoc view '{}'", read.replace('\\', "/")),
        ),
        ("PowerShell", format!("stetdoc view '{read}'")),
    ] {
        let hook = fx.hook(tool, &json!({"command": command}));
        let got = guard_with(&args, hook.to_string().as_bytes());
        assert!(got.contains("was denied"), "{tool} {command}: {got}");
        assert!(
            got.contains(&format!("({other})")),
            "{tool} {command}: {got}"
        );
    }
}

/// The data folder written as its real path and with `\\?\`, in the fence or in the read,
/// either way round: refused as the data folder's.
#[cfg(windows)]
#[test]
fn the_data_folders_real_and_verbatim_paths_are_refused() {
    let fx = Fixture::new("stetdoc-data-forms");
    crate::write::create_dir_all(&fx.root.join("data").join("logs")).unwrap();
    crate::write::write_file(&fx.root.join(r"data\logs\host.docx"), b"x").unwrap();
    let real = dunce::canonicalize(fx.root.join("data")).unwrap();
    let real = real.to_str().unwrap().to_owned();
    let verbatim = format!(r"\\?\{real}");
    let matter = fx.path("m");
    for (fenced, read) in [
        (fx.path("data"), real.clone()),
        (fx.path("data"), verbatim.clone()),
        (verbatim.clone(), real.clone()),
        (real.clone(), verbatim.clone()),
    ] {
        let args = ["--matter", &matter, "--fence-data", &fenced];
        let read = format!(r"{read}\logs\host.docx");
        let command = format!("stetdoc view '{read}'");
        let hook = fx.hook("PowerShell", &json!({"command": command}));
        let got = guard_with(&args, hook.to_string().as_bytes());
        assert!(got.contains(DATA_SAID), "{fenced} {command}: {got}");
    }
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
    let real = std::fs::canonicalize(fx.root.join("data")).unwrap();
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
        assert!(got.contains(DATA_SAID), "{fenced} {command}: {got}");
    }
}
