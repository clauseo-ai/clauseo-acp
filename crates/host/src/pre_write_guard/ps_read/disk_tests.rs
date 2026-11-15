//! The allowlist's path checks on a fake disk (`FakeFs`, each look recorded) and once through the
//! whole guard on the real disk. Paths are written `/m/…` and made absolute for this OS
//! ([`abs`]), and the line uses this OS's rules, so the same cases run on Windows (with Windows'
//! path rules) and on Unix.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::parse::PIN;
use super::{Folders, Look, MAX_ENTRIES, REASON, Tree, answer};
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::tests::{Fake, FakeFs};
use crate::network_paths::resolve::{Disk, Entry};
use crate::network_paths::tests::plain_net;
use crate::testing::abs;

impl Tree for FakeFs {
    fn children(&self, dir: &Path, limit: usize) -> io::Result<Vec<(OsString, Entry)>> {
        let mut out = Vec::new();
        for (path, kind) in &self.names {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                continue;
            };
            if parent != dir {
                continue;
            }
            if out.len() == limit {
                return Err(io::Error::other("too many"));
            }
            let entry = match kind {
                Fake::Dir => Entry::Dir,
                Fake::File => Entry::File,
                Fake::Link(_) | Fake::BadLink => Entry::Link,
                Fake::Denied | Fake::BadName => return Err(io::ErrorKind::PermissionDenied.into()),
            };
            out.push((name.to_owned(), entry));
        }
        Ok(out)
    }
}

/// The matter `/m`, `<data>` `/d` with this session's work and notes folders, another matter's
/// notes and the files the fence denies, and `/out` outside them all.
fn disk() -> FakeFs {
    let names = [
        ("/m/a.txt", Fake::File),
        ("/m/Drafts/b.docx", Fake::File),
        ("/m/sub/c.txt", Fake::File),
        ("/d/state.sqlite", Fake::File),
        ("/d/run/r1/relay-token", Fake::File),
        ("/d/run/r1/work/w.txt", Fake::File),
        ("/d/agent-state/m1/notes/n.md", Fake::File),
        ("/d/agent-state/m2/notes/other.md", Fake::File),
        ("/out/secret.txt", Fake::File),
    ];
    let owned: Vec<(String, Fake)> = names
        .into_iter()
        .map(|(name, kind)| (abs(name), kind))
        .collect();
    with(&owned)
}

fn with(names: &[(String, Fake)]) -> FakeFs {
    let names: Vec<(&str, Fake)> = names
        .iter()
        .map(|(name, kind)| (name.as_str(), kind.clone()))
        .collect();
    FakeFs::with(&names)
}

/// Adds `name` (made absolute) to `fs`, as `kind`.
fn add(fs: &mut FakeFs, name: &str, kind: Fake) {
    fs.names.insert(PathBuf::from(abs(name)), kind);
}

fn folders() -> Folders {
    Folders {
        data: vec![abs("/d").into()],
        work: abs("/d/run/r1/work").into(),
        notes: abs("/d/agent-state/m1/notes").into(),
    }
}

/// `text` with `/` as this OS writes a path.
fn os(text: &str) -> String {
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text.to_owned()
    }
}

/// A case: the disk, the canonical names (`real`), the matter, the folders and the `cwd`.
struct Case {
    fs: FakeFs,
    real: BTreeMap<PathBuf, PathBuf>,
    matter: PathBuf,
    folders: Folders,
    cwd: Option<String>,
}

impl Case {
    fn new() -> Self {
        Self {
            fs: disk(),
            real: BTreeMap::new(),
            matter: abs("/m").into(),
            folders: folders(),
            cwd: Some(abs("/m")),
        }
    }

    /// The hook's answer for `input` (a PowerShell tool input).
    fn answer_input(&self, input: &Value) -> Option<Value> {
        let net: NetEnv = plain_net(cfg!(windows));
        let real = |path: &Path| self.real.get(path).cloned();
        let look = Look {
            disk: Disk {
                fs: &self.fs,
                real: &real,
                net: &net,
            },
            tree: &self.fs,
        };
        let mut hook = json!({
            "session_id": "s",
            "hook_event_name": "PreToolUse",
            "tool_name": "PowerShell",
            "tool_input": input,
        });
        if let Some(cwd) = &self.cwd {
            hook["cwd"] = Value::from(cwd.as_str());
        }
        let out = answer(&hook, &self.matter, &self.folders, &look)?;
        Some(serde_json::from_str(&out).unwrap())
    }

    /// The rewritten command for `line` (paths in it written `/m/…`), if allowed.
    fn command(&self, line: &str) -> Option<String> {
        let line = absolute(line);
        let out = self.answer_input(&json!({"command": line, "description": "d"}))?;
        let specific = &out["hookSpecificOutput"];
        assert_eq!(specific["permissionDecision"], "allow");
        assert_eq!(specific["permissionDecisionReason"], REASON);
        assert_eq!(specific["updatedInput"]["description"], "d");
        Some(
            specific["updatedInput"]["command"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
    }
}

/// `line` with each `@/x` made the absolute path of `/x` on this OS.
fn absolute(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '@' {
            out.push(c);
            continue;
        }
        let mut path = String::new();
        while let Some(next) = chars.next_if(|next| !['"', '\'', ' ', ';'].contains(next)) {
            path.push(next);
        }
        out.push_str(&abs(&path));
    }
    out
}

fn gci(path: &str, rest: &str) -> String {
    format!(
        "Microsoft.PowerShell.Management\\Get-ChildItem -LiteralPath '{}'{rest}",
        abs(path)
    )
}

fn gc(path: &str, rest: &str) -> String {
    format!(
        "Microsoft.PowerShell.Management\\Get-Content -LiteralPath '{}'{rest}",
        abs(path)
    )
}

#[test]
fn line_b_on_the_matter_and_the_own_notes_is_allowed_and_rewritten() {
    let case = Case::new();
    let line = r#"Get-ChildItem -Recurse -File "@/m" | Select-Object FullName, Length; Get-ChildItem "@/d/agent-state/m1/notes""#;
    let want = format!(
        "{PIN}{} | Microsoft.PowerShell.Utility\\Select-Object -Property FullName, Length; {}",
        gci("/m", " -Recurse -File"),
        gci("/d/agent-state/m1/notes", "")
    );
    assert_eq!(case.command(line), Some(want));
}

#[test]
fn reads_of_the_matter_and_own_folders_are_allowed() {
    let case = Case::new();
    let rel = os("./a.txt");
    for (line, want) in [
        ("gci".to_owned(), gci("/m", "")),
        (format!("gc {rel} -Tail 2"), gc("/m/a.txt", " -Tail 2")),
        (
            "gc '@/m/Drafts/b.docx' -Raw".to_owned(),
            gc("/m/Drafts/b.docx", " -Raw"),
        ),
        (
            "gci @/m/sub/c.txt -Recurse".to_owned(),
            gci("/m/sub/c.txt", " -Recurse"),
        ),
        (
            "dir @/d/run/r1/work -Name".to_owned(),
            gci("/d/run/r1/work", " -Name"),
        ),
        (
            "gc @/d/agent-state/m1/notes/n.md".to_owned(),
            gc("/d/agent-state/m1/notes/n.md", ""),
        ),
    ] {
        assert_eq!(case.command(&line), Some(format!("{PIN}{want}")), "{line}");
    }
}

#[test]
fn reads_outside_the_qualifying_folders_get_no_answer() {
    let case = Case::new();
    for line in [
        "gc @/out/secret.txt",
        "gci @/out",
        "gc @/d/state.sqlite",
        "gci @/d",
        "gci @/d/run/r1",
        "gc @/d/run/r1/relay-token",
        "gci @/d/agent-state",
        "gc @/d/agent-state/m2/notes/other.md",
        "gci @/d/agent-state/m2/notes",
        "gc @/m/missing.txt",
        "gc @/m/sub",
        "gci @/m/sub; gc @/out/secret.txt",
    ] {
        assert_eq!(case.command(line), None, "{line}");
    }
}

/// A link or junction below a qualifying folder refuses, wherever it points; one above the
/// matter (a junctioned Documents folder) is fine, and the canonical path is what runs.
#[test]
fn links_below_the_root_refuse_and_links_above_it_resolve() {
    let mut case = Case::new();
    add(&mut case.fs, "/m/j", Fake::Link(abs("/d").into()));
    add(
        &mut case.fs,
        "/m/sub/l",
        Fake::Link(abs("/m/Drafts").into()),
    );
    add(&mut case.fs, "/alias", Fake::Link(abs("/m").into()));
    for line in [
        "gc @/m/j/state.sqlite",
        "gci @/m/j",
        "gci @/m -Recurse",
        "gci @/m",
        "gc @/m/sub/l/b.docx",
        "gci @/m/sub",
        "gci @/alias -Recurse",
    ] {
        assert_eq!(case.command(line), None, "{line}");
    }
    assert_eq!(
        case.command("gc @/alias/a.txt"),
        Some(format!("{PIN}{}", gc("/m/a.txt", "")))
    );
    assert_eq!(
        case.command("gci @/m/Drafts -Recurse"),
        Some(format!("{PIN}{}", gci("/m/Drafts", " -Recurse")))
    );
}

/// Another case and an 8.3 name are walked to the canonical name, which runs; the same tricks
/// on `<data>` still land outside.
#[test]
fn other_case_and_short_names_run_as_the_canonical_path() {
    let mut case = Case::new();
    add(&mut case.fs, "/M", Fake::Dir);
    add(&mut case.fs, "/m/CONTRA~1.TXT", Fake::File);
    add(&mut case.fs, "/m/Contract.txt", Fake::File);
    add(&mut case.fs, "/D", Fake::Dir);
    for (spelled, canonical) in [
        ("/M", "/m"),
        ("/m/CONTRA~1.TXT", "/m/Contract.txt"),
        ("/D", "/d"),
    ] {
        case.real.insert(abs(spelled).into(), abs(canonical).into());
    }
    assert_eq!(
        case.command("gc '@/M/CONTRA~1.TXT'"),
        Some(format!("{PIN}{}", gc("/m/Contract.txt", "")))
    );
    assert_eq!(case.command("gc @/D/state.sqlite"), None);
}

/// A name the walk can't vouch for (an unknown reparse point answers as an error), in the
/// path or in a listed folder, refuses; so does a listing past [`MAX_ENTRIES`].
#[test]
fn unknown_entries_and_huge_listings_refuse() {
    let mut case = Case::new();
    add(&mut case.fs, "/m/odd", Fake::Dir);
    add(&mut case.fs, "/m/odd/x", Fake::Denied);
    add(&mut case.fs, "/m/odd2", Fake::Denied);
    for line in ["gci @/m/odd", "gc @/m/odd2/y", "gci @/m"] {
        assert_eq!(case.command(line), None, "{line}");
    }
    let mut big = Case::new();
    add(&mut big.fs, "/m/big", Fake::Dir);
    for n in 0..=MAX_ENTRIES {
        add(&mut big.fs, &format!("/m/big/f{n}"), Fake::File);
    }
    assert_eq!(big.command("gci @/m/big"), None);
    assert!(big.command("gci @/m/sub").is_some());
}

/// The matter qualifies only when it is a named local folder clear of `<data>`; the own
/// folders only at their place under `<data>`, never through a link.
#[test]
fn only_qualifying_folders_count() {
    let mut inside = Case::new();
    add(&mut inside.fs, "/d/mm", Fake::Dir);
    add(&mut inside.fs, "/d/mm/x.txt", Fake::File);
    inside.matter = abs("/d/mm").into();
    inside.cwd = Some(abs("/d/mm"));
    assert_eq!(inside.command("gc @/d/mm/x.txt"), None);
    assert_eq!(inside.command("gci"), None);
    assert!(inside.command("gc @/d/agent-state/m1/notes/n.md").is_some());
    let mut holding = Case::new();
    holding.matter = abs("/").into();
    assert_eq!(holding.command("gc @/m/a.txt"), None);
    let mut linked = Case::new();
    add(
        &mut linked.fs,
        "/d/run/r1/work",
        Fake::Link(abs("/out").into()),
    );
    assert_eq!(linked.command("gc @/d/run/r1/work/secret.txt"), None);
    let mut misplaced = Case::new();
    misplaced.folders.notes = abs("/d/agent-state/m1/notes/deeper").into();
    assert_eq!(misplaced.command("gc @/d/agent-state/m1/notes/n.md"), None);
    let mut no_data = Case::new();
    no_data.folders.data.clear();
    assert_eq!(no_data.command("gc @/m/a.txt"), None);
}

/// A relative path only from a `cwd` that is the matter; other tools, input keys and folder
/// kinds get no answer.
#[test]
fn shape_cwd_and_kind_rules() {
    let mut case = Case::new();
    let rel = format!("gc {}", os("./a.txt"));
    case.cwd = Some(abs("/m/sub"));
    assert_eq!(case.command(&rel), None);
    assert_eq!(case.command("gci"), None);
    assert!(case.command("gc @/m/a.txt").is_some());
    case.cwd = None;
    assert_eq!(case.command(&rel), None);
    let case = Case::new();
    let input = json!({"command": "gci", "dangerouslyDisableSandbox": true});
    assert_eq!(case.answer_input(&input), None);
    assert_eq!(case.answer_input(&json!({"command": 1})), None);
    let kept = json!({"command": "gci", "timeout": 5, "run_in_background": false});
    let out = case.answer_input(&kept).unwrap();
    assert_eq!(out["hookSpecificOutput"]["updatedInput"]["timeout"], 5);
    assert_eq!(
        out["hookSpecificOutput"]["updatedInput"]["run_in_background"],
        false
    );
    assert_eq!(case.command("gc @/m/Drafts"), None);
    assert_eq!(case.command("gc @/m/a.txt; gc @/m/sub"), None);
}

/// Windows' own rules: reserved device names, a stream and a path past 259 characters refuse.
#[cfg(windows)]
#[test]
fn windows_names_and_long_paths_refuse() {
    let mut case = Case::new();
    let long = format!("/m/{}", "a".repeat(250));
    add(&mut case.fs, &long, Fake::Dir);
    add(&mut case.fs, &format!("{long}/x.txt"), Fake::File);
    add(&mut case.fs, "/m/CON", Fake::File);
    for line in ["gc @/m/CON", "gc @/m/NUL.txt", "gc '@/m/a.txt.'"] {
        assert_eq!(case.command(line), None, "{line}");
    }
    assert_eq!(case.command(&format!("gc @{long}/x.txt")), None);
}

/// Through the whole guard on the real disk: the allow and its rewrite on stdout, exit 0;
/// nothing without `--ps-read` or for an outside path; forced ask's `ask` instead under
/// `--ask-all`.
#[test]
fn the_guard_allows_a_real_read_and_rewrites_it() {
    let root = std::env::temp_dir().join(format!("clauseo-ps-read-{}", std::process::id()));
    crate::write::remove_dir_all(&root).unwrap();
    for folder in ["m", "d/run/r1/work", "d/agent-state/m1/notes", "out"] {
        crate::write::create_dir_all(&root.join(folder)).unwrap();
    }
    for file in ["m/a.txt", "out/s.txt", "d/state.sqlite"] {
        crate::write::write_file(&root.join(file), b"x").unwrap();
    }
    let at = |name: &str| root.join(name).to_str().unwrap().to_owned();
    let mut args = vec!["--matter".to_owned(), at("m"), "--ps-read".to_owned()];
    args.extend([
        "--ps-work".to_owned(),
        at("d/run/r1/work"),
        "--ps-notes".to_owned(),
    ]);
    args.extend([
        at("d/agent-state/m1/notes"),
        "--ps-data".to_owned(),
        at("d"),
    ]);
    let run = |args: &[String], file: &str| {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let hook = json!({
            "cwd": at("m"),
            "hook_event_name": "PreToolUse",
            "tool_name": "PowerShell",
            "tool_input": {"command": format!("Get-Content '{}'", at(file))},
        });
        let (mut out, mut errors) = (Vec::new(), Vec::new());
        let code = super::super::run_timed(
            &args,
            hook.to_string().as_bytes(),
            (&mut out, &mut errors),
            false,
        );
        assert_eq!((code, errors.as_slice()), (0, &b""[..]));
        String::from_utf8(out).unwrap()
    };
    let canonical = dunce::canonicalize(root.join("m").join("a.txt")).unwrap();
    let want = format!(
        "{PIN}Microsoft.PowerShell.Management\\Get-Content -LiteralPath '{}'",
        canonical.display()
    );
    let out: Value = serde_json::from_str(&run(&args, "m/a.txt")).unwrap();
    assert_eq!(
        out["hookSpecificOutput"]["updatedInput"]["command"],
        want.as_str()
    );
    assert_eq!(run(&args, "out/s.txt"), "");
    assert_eq!(run(&args[..2], "m/a.txt"), "");
    let mut ask = args[..2].to_vec();
    ask.push("--ask-all".to_owned());
    ask.extend(args[2..].iter().cloned());
    assert!(run(&ask, "m/a.txt").contains(r#""permissionDecision":"ask""#));
    #[cfg(unix)]
    {
        crate::write::symlink(&root.join("d"), &root.join("m").join("j")).unwrap();
        assert_eq!(run(&args, "m/j/state.sqlite"), "");
        assert!(
            !run(&args, "m/a.txt").is_empty(),
            "a link elsewhere in the matter"
        );
    }
    crate::write::remove_dir_all(&root).unwrap();
}
