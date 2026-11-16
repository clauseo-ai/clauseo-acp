//! The stetdoc read fence on a real Windows disk, through the guard: junctions and directory
//! links, 8.3 names, verbatim, long, device and UNC paths, case, trailing dots and spaces, Git
//! Bash's path forms and a folder change. Temp folders only; the guard's decision is read, never
//! a document.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::{Fixture, guard_with};

/// What the guard says of a line that is not the plain shape, and of a word it can't follow.
const PLAIN: &str = "stetdoc runs only as a plain command";
/// What it says of another matter's document.
const DENIED: &str = "was denied";
/// What it says of the data folder's.
const DATA_SAID: &str = "it is in Clauseo's own data folder";

/// A fixture with a data folder (`data`, holding `logs\host.docx`) beside the matter `m` and the
/// other matter `out`, and the guard's arguments fencing both.
struct Fenced {
    fx: Fixture,
    args: Vec<String>,
}

impl Fenced {
    fn new(test: &str) -> Self {
        let fx = Fixture::new(test);
        crate::write::create_dir_all(&fx.root.join(r"data\logs")).unwrap();
        crate::write::write_file(&fx.root.join(r"data\logs\host.docx"), b"x").unwrap();
        let args = [
            "--matter",
            &fx.path("m"),
            "--fence-matter",
            &fx.path("out"),
            "--fence-data",
            &fx.path("data"),
        ]
        .map(str::to_owned)
        .to_vec();
        Self { fx, args }
    }

    /// What the guard says of `command` run by `tool` from the matter.
    fn said(&self, tool: &str, command: &str) -> String {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        let hook = self.fx.hook(tool, &json!({"command": command}));
        guard_with(&args, hook.to_string().as_bytes())
    }

    /// Asserts each Bash and PowerShell command is refused with `want` in the reason, and never
    /// with the other matter's folder.
    fn refused(&self, commands: &[(&str, String)], want: &str) {
        let other = self.fx.path("out");
        for (tool, command) in commands {
            let said = self.said(tool, command);
            assert!(said.contains(want), "{tool} {command}: {said}");
            assert!(!said.contains(&other), "{tool} {command}: {said}");
        }
    }
}

/// `path` as a Bash word: forward slashes, single-quoted.
fn bash_word(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\\', "/"))
}

/// `path` as a PowerShell word, single-quoted.
fn pwsh_word(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap())
}

/// `stetdoc view` of `path` in both shells.
fn both(path: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("Bash", format!("stetdoc view {}", bash_word(path))),
        ("PowerShell", format!("stetdoc view {}", pwsh_word(path))),
    ]
}

/// Makes `link` a directory symbolic link to `target`; `false`, said loudly, without the right
/// to make links (no Developer Mode, no admin).
fn dir_link(target: &Path, link: &Path) -> bool {
    match crate::write::symlink(target, link) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("SKIPPED a directory link check: {error}");
            false
        }
    }
}

/// The Windows copy of the end-to-end test: another matter's document through a junction in
/// the matter is refused in both shells without naming its folder; a template reads.
#[test]
fn another_matter_is_refused_and_a_template_reads_on_windows() {
    let fenced = Fenced::new("stetdoc-win-e2e");
    let fx = &fenced.fx;
    crate::write::create_dir_all(&fx.root.join("docs")).unwrap();
    crate::write::write_file(&fx.root.join(r"docs\template.docx"), b"x").unwrap();
    let junction = clauseo_sandbox::windows::test_junction::junction;
    junction(&fx.matter().join("link"), &fx.root.join("out")).unwrap();
    fenced.refused(
        &[
            ("Bash", "stetdoc view link/Held.docx".to_owned()),
            ("PowerShell", r"stetdoc view link\Held.docx".to_owned()),
        ],
        DENIED,
    );
    for (tool, command) in both(&fx.root.join(r"docs\template.docx")) {
        assert_eq!(fenced.said(tool, &command), "", "{tool} {command}");
    }
}

/// A junction into another matter and into the data folder, and a directory link with a
/// relative target into another matter, are refused.
#[test]
fn a_junction_or_a_directory_link_out_of_the_matter_is_refused() {
    let fenced = Fenced::new("stetdoc-win-junction");
    let fx = &fenced.fx;
    let junction = clauseo_sandbox::windows::test_junction::junction;
    junction(&fx.matter().join("toB"), &fx.root.join("out")).unwrap();
    junction(&fx.matter().join("toData"), &fx.root.join("data")).unwrap();
    fenced.refused(
        &[
            ("Bash", "stetdoc view toB/Held.docx".to_owned()),
            ("PowerShell", r"stetdoc view toB\Held.docx".to_owned()),
        ],
        DENIED,
    );
    fenced.refused(
        &[
            ("Bash", "stetdoc view toData/logs/host.docx".to_owned()),
            (
                "PowerShell",
                r"stetdoc view toData\logs\host.docx".to_owned(),
            ),
        ],
        DATA_SAID,
    );
    if dir_link(Path::new(r"..\out"), &fx.matter().join("rel")) {
        fenced.refused(&[("Bash", "stetdoc view rel/Held.docx".to_owned())], DENIED);
    }
    if dir_link(&fx.root.join("data"), &fx.matter().join("dataLink")) {
        fenced.refused(
            &[(
                "PowerShell",
                r"stetdoc view dataLink\logs\host.docx".to_owned(),
            )],
            DATA_SAID,
        );
    }
}

/// The 8.3 short names of another matter's long-named folder and of a long-named data folder
/// are those folders. Short names must be on for the temp volume: the test fails, not skips,
/// when they are off.
#[test]
fn the_short_names_of_fenced_folders_are_refused() {
    let fx = Fixture::new("stetdoc-win-short");
    let other = fx.root.join("Another Matter Folder");
    let data = fx.root.join("Clauseo Data Folder");
    crate::write::create_dir_all(&other).unwrap();
    crate::write::write_file(&other.join("Held.docx"), b"x").unwrap();
    crate::write::create_dir_all(&data.join("logs")).unwrap();
    crate::write::write_file(&data.join(r"logs\host.docx"), b"x").unwrap();
    let short = |long: &PathBuf| {
        crate::network_paths::short_path(long)
            .filter(|short| short != long)
            .unwrap_or_else(|| panic!("8.3 short names are off for {}", long.display()))
    };
    let (other_short, data_short) = (short(&other), short(&data));
    let matter = fx.path("m");
    let (other, data) = (other.to_str().unwrap(), data.to_str().unwrap());
    let args = [
        "--matter",
        &matter,
        "--fence-matter",
        other,
        "--fence-data",
        data,
    ];
    for (path, want) in [
        (other_short.join("Held.docx"), DENIED),
        (data_short.join(r"logs\host.docx"), DATA_SAID),
    ] {
        for (tool, command) in both(&path) {
            let hook = fx.hook(tool, &json!({"command": command}));
            let said = guard_with(&args, hook.to_string().as_bytes());
            assert!(said.contains(want), "{tool} {command}: {said}");
            assert!(!said.contains(other), "{tool} {command}: {said}");
        }
    }
}

/// The data folder's verbatim spelling in the fence still fences its plain path; a verbatim,
/// device or `GLOBALROOT` path in a stetdoc line is refused; so is a long path past 260
/// characters into the data folder; and a UNC path (that no disk is looked at for one is
/// `reads::tests::unc`).
#[test]
fn verbatim_device_long_and_unc_paths_are_refused() {
    let fx = Fixture::new("stetdoc-win-verbatim");
    crate::write::create_dir_all(&fx.root.join(r"data\logs")).unwrap();
    crate::write::write_file(&fx.root.join(r"data\logs\host.docx"), b"x").unwrap();
    let real = dunce::canonicalize(fx.root.join("data")).unwrap();
    let verbatim = format!(r"\\?\{}", real.display());
    let matter = fx.path("m");
    let args = ["--matter", &matter, "--fence-data", &verbatim];
    let said = |tool: &str, command: &str| {
        let hook = fx.hook(tool, &json!({"command": command}));
        guard_with(&args, hook.to_string().as_bytes())
    };
    for (tool, command) in both(&real.join(r"logs\host.docx")) {
        let got = said(tool, &command);
        assert!(got.contains(DATA_SAID), "{tool} {command}: {got}");
    }
    let drive = real.to_str().unwrap().get(..2).unwrap().to_owned();
    for word in [
        format!(r"'{verbatim}\logs\host.docx'"),
        format!(r"'\\.\{}\logs\host.docx'", real.display()),
        format!(
            r"'\\?\GLOBALROOT\Device\HarddiskVolume1{}\x.docx'",
            real.to_str().unwrap().get(2..).unwrap()
        ),
        format!(r"'\\.\{drive}\x.docx'"),
    ] {
        let got = said("PowerShell", &format!("stetdoc view {word}"));
        assert!(got.contains(PLAIN), "{word}: {got}");
    }
    let mut long = real;
    while long.as_os_str().len() < 300 {
        long.push("a-long-folder-name-for-the-path-limit");
    }
    crate::write::create_dir_all(&long).unwrap();
    crate::write::write_file(&long.join("deep.docx"), b"x").unwrap();
    for (tool, command) in both(&long.join("deep.docx")) {
        let got = said(tool, &command);
        assert!(got.contains(DATA_SAID), "{tool} {command}: {got}");
    }
    for word in [
        r"'\\clauseo-nowhere.invalid\share\x.docx'",
        r"\\clauseo-nowhere.invalid\share\x.docx",
    ] {
        let got = said("PowerShell", &format!("stetdoc view {word}"));
        assert!(got.contains(PLAIN), "{word}: {got}");
    }
    let got = said(
        "Bash",
        "stetdoc view //clauseo-nowhere.invalid/share/x.docx",
    );
    assert!(got.contains(PLAIN), "{got}");
}

/// The fenced folders written in another case, the drive letter too, are refused.
#[test]
fn another_case_of_a_fenced_folder_is_refused() {
    let fenced = Fenced::new("stetdoc-win-case");
    let fx = &fenced.fx;
    let case = |path: PathBuf| {
        let text = path.to_str().unwrap();
        let (drive, rest) = text.split_at(1);
        PathBuf::from(format!("{}{}", drive.to_lowercase(), rest.to_uppercase()))
    };
    fenced.refused(&both(&case(fx.root.join(r"out\Held.docx"))), DENIED);
    fenced.refused(
        &both(&case(fx.root.join(r"data\logs\host.docx"))),
        DATA_SAID,
    );
}

/// A fenced folder written with a trailing dot or space, which Windows drops, is refused.
#[test]
fn a_fenced_folder_with_a_trailing_dot_or_space_is_refused() {
    let fenced = Fenced::new("stetdoc-win-trailing");
    let fx = &fenced.fx;
    for name in [r"out.\Held.docx", r"out \Held.docx"] {
        for (tool, command) in both(&fx.root.join(name)) {
            let said = fenced.said(tool, &command);
            assert!(
                said.contains(DENIED) || said.contains(PLAIN),
                "{tool} {command}: {said}"
            );
        }
    }
}

/// Git Bash's drive forms of a fenced folder are read as Git Bash hands them on and refused;
/// its other root paths are refused as unreadable.
#[test]
fn git_bash_path_forms_are_refused() {
    let fenced = Fenced::new("stetdoc-win-git-bash");
    let held = dunce::canonicalize(fenced.fx.root.join(r"out\Held.docx")).unwrap();
    let text = held.to_str().unwrap().replace('\\', "/");
    let (drive, rest) = text.split_at(1);
    let rest = rest.trim_start_matches(':');
    let drive = drive.to_lowercase();
    for word in [
        format!("/{drive}{rest}"),
        format!("/cygdrive/{drive}{rest}"),
        format!("/proc/cygdrive/{drive}{rest}"),
    ] {
        fenced.refused(&[("Bash", format!("stetdoc view {word}"))], DENIED);
    }
    fenced.refused(&[("Bash", "stetdoc view /tmp/x.docx".to_owned())], PLAIN);
}

/// A folder change in a stetdoc line, in either shell, is refused before a relative path.
#[test]
fn a_folder_change_then_a_relative_path_is_refused() {
    let fenced = Fenced::new("stetdoc-win-cd");
    let out = fenced.fx.path("out");
    let mut commands = Vec::new();
    for change in ["Set-Location", "sl", "pushd", "cd"] {
        commands.push((
            "PowerShell",
            format!("{change} '{out}'; stetdoc view Held.docx"),
        ));
    }
    for change in ["cd", "pushd"] {
        let out = out.replace('\\', "/");
        commands.push((
            "Bash",
            format!("{change} '{out}' && stetdoc view Held.docx"),
        ));
    }
    fenced.refused(&commands, PLAIN);
}
