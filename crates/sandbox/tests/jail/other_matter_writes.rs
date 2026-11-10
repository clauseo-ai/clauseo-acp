//! Writes into another matter (R18) under the real `sandbox-exec`: a read grant of it, of a
//! folder in it or of one file in it reads and never writes. No file or folder is made,
//! overwritten, appended to, moved in, out or within, removed, or given new modes, flags, times
//! or extended attributes, at the grant's root, in a folder below it, or beside a nested `.git`;
//! and no agent instruction file or folder is planted there. Then each write allowance the
//! profile has is pointed at the other matter: the policy refuses to build, or the jail still
//! refuses the writes.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clauseo_sandbox::{
    BrowserSocketDir, Error, HiddenOverlapAllowance, Policy, PolicyBuilder, SeatbeltLauncher,
};

use crate::support::{World, launcher, run_outside, sh};

/// The writes [`WRITES`] tries, by the label it prints.
const WRITE_OPS: [&str; 20] = [
    "append",
    "chflags",
    "chmod",
    "copy-in",
    "create",
    "delete",
    "hardlink-in",
    "mkdir",
    "mkdir-deep",
    "move-folder-out",
    "move-in",
    "move-out",
    "overwrite",
    "rename",
    "rename-folder",
    "rmdir",
    "symlink",
    "touch",
    "truncate",
    "xattr",
];

/// Tries each write on `$1`, a folder of the other matter holding `doc.txt` and `empty/`, with
/// `$2`, a folder of this matter holding `in.txt`, as the other end of moves and links. Prints
/// `DONE <op>` or `REFUSED <op>` for each of [`WRITE_OPS`].
const WRITES: &str = r#"t="$1"; m="$2"
try() { if ( eval "$2" ) >/dev/null 2>&1; then echo "DONE $1"; else echo "REFUSED $1"; fi; }
try create 'printf x > "$t/new.txt"'
try mkdir 'mkdir "$t/newdir"'
try mkdir-deep 'mkdir -p "$t/a/b"'
try overwrite 'printf x > "$t/doc.txt"'
try truncate ': > "$t/doc.txt" && test ! -s "$t/doc.txt"'
try append 'printf x >> "$t/doc.txt"'
try move-in 'mv "$m/in.txt" "$t/in.txt"'
try copy-in 'cp "$m/in.txt" "$t/copy.txt"'
try hardlink-in 'ln "$m/in.txt" "$t/hard.txt"'
try symlink 'ln -s /etc/hosts "$t/link"'
try delete 'rm -f "$t/doc.txt" && test ! -e "$t/doc.txt"'
try rename 'mv "$t/doc.txt" "$t/renamed.txt"'
try move-out 'mv "$t/doc.txt" "$m/out.txt"'
try rmdir 'rmdir "$t/empty"'
try rename-folder 'mv "$t/empty" "$t/empty2"'
try move-folder-out 'mv "$t/empty" "$m/empty-out"'
try chmod 'chmod 600 "$t/doc.txt"'
try chflags 'chflags nodump "$t/doc.txt"'
try xattr '/usr/bin/xattr -w com.example.k v "$t/doc.txt"'
try touch 'touch -t 200001010000 "$t/doc.txt"'
exit 0"#;

/// The plants [`PLANTS`] tries, by the label it prints.
const PLANT_OPS: [&str; 10] = [
    "agents-dir",
    "agents-md",
    "claude-dir",
    "claude-md",
    "claude-md-case",
    "claude-settings",
    "codex-dir",
    "move-in-claude-dir",
    "move-in-claude-md",
    "move-in-claude-md-dir",
];

/// The names [`PLANTS`] would make in a folder: none may exist afterwards.
const PLANTED: [&str; 7] = [
    "CLAUDE.md",
    "Claude.MD",
    "AGENTS.md",
    ".claude",
    ".codex",
    ".agents",
    "CLAUDE.md.d",
];

/// Tries to plant agent instruction files and folders in `$1`, a folder of the other matter,
/// with `$2`, a folder of this matter holding `plant.md` and `plant-dir/settings.json`, as the
/// source of moves. Prints `DONE <op>` or `REFUSED <op>` for each of [`PLANT_OPS`].
const PLANTS: &str = r#"t="$1"; m="$2"
try() { if ( eval "$2" ) >/dev/null 2>&1; then echo "DONE $1"; else echo "REFUSED $1"; fi; }
try claude-md 'printf x > "$t/CLAUDE.md"'
try claude-md-case 'printf x > "$t/Claude.MD"'
try agents-md 'printf x > "$t/AGENTS.md"'
try claude-dir 'mkdir "$t/.claude"'
try claude-settings 'mkdir -p "$t/.claude"; printf "{}" > "$t/.claude/settings.json"'
try codex-dir 'mkdir "$t/.codex"'
try agents-dir 'mkdir "$t/.agents"'
try move-in-claude-md 'mv "$m/plant.md" "$t/CLAUDE.md"'
try move-in-claude-dir 'mv "$m/plant-dir" "$t/.claude"'
try move-in-claude-md-dir 'mv "$m/plant-dir" "$t/CLAUDE.md.d"'
exit 0"#;

/// One entry of a folder as the test sees it: a folder or a file's bytes, its mode, flags and
/// modification time.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    bytes: Option<Vec<u8>>,
    mode: u32,
    flags: u32,
    modified: SystemTime,
}

/// Every entry under `dir`, and `dir` itself, by path, with the extended attributes of the tree
/// as `xattr -lr` lists them.
fn snapshot(dir: &Path) -> (BTreeMap<PathBuf, Seen>, String) {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(path) = stack.pop() {
        let meta = std::fs::symlink_metadata(&path).unwrap();
        let bytes = if meta.is_dir() {
            for entry in std::fs::read_dir(&path).unwrap() {
                stack.push(entry.unwrap().path());
            }
            None
        } else {
            Some(std::fs::read(&path).unwrap_or_default())
        };
        let seen = Seen {
            bytes,
            mode: meta.mode(),
            flags: std::os::macos::fs::MetadataExt::st_flags(&meta),
            modified: meta.modified().unwrap(),
        };
        out.insert(path, seen);
    }
    let xattrs = run_outside("/usr/bin/xattr", &["-lr", dir.to_str().unwrap()], dir);
    (out, xattrs.stdout)
}

/// The other matter at `root` (relative): `doc.txt` and `empty/` at its root, in `sub/` (which
/// holds a `.git`) and in `sub/deeper/` (which holds a `.git` of its own); and in `m1` the
/// sources of the moves and links.
fn other_matter(w: &World, root: &str) {
    for dir in ["", "/sub", "/sub/deeper"] {
        w.file(&format!("{root}{dir}/doc.txt"), "doc\n");
        w.dir(&format!("{root}{dir}/empty"));
    }
    w.file(&format!("{root}/sub/.git/HEAD"), "ref\n");
    w.file(&format!("{root}/sub/deeper/.git/HEAD"), "ref\n");
    w.file("m1/in.txt", "in\n");
    w.file("m1/plant.md", "plant\n");
    w.file("m1/plant-dir/settings.json", "{}\n");
}

/// The labels after `word` in `stdout`, sorted.
fn lines(stdout: &str, word: &str) -> Vec<String> {
    let mut out: Vec<String> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix(word))
        .map(|rest| rest.trim().to_owned())
        .collect();
    out.sort();
    out
}

/// Runs `script` on the folder `target` from `m1` and asserts every one of `ops` was refused.
#[track_caller]
fn all_refused(w: &World, launcher: &SeatbeltLauncher, script: &str, ops: &[&str], target: &Path) {
    let m1 = w.at("m1");
    let out = sh(
        launcher,
        &m1,
        script,
        &[target.to_str().unwrap(), m1.to_str().unwrap()],
    );
    out.assert_ok();
    assert_eq!(
        lines(&out.stdout, "DONE "),
        Vec::<String>::new(),
        "writes in {}: {out:#?}",
        target.display()
    );
    assert_eq!(lines(&out.stdout, "REFUSED "), ops, "{out:#?}");
}

/// Under `launcher`, tries [`WRITES`] in each of `writes` and [`PLANTS`] in each of `plants`
/// (relative folders of the other matter at `root`), and asserts each was refused and the other
/// matter and the move sources in `m1` are as they were.
#[track_caller]
fn assert_read_only(
    w: &World,
    launcher: &SeatbeltLauncher,
    root: &str,
    writes: &[&str],
    plants: &[&str],
) {
    let before = snapshot(&w.at(root));
    for dir in writes {
        all_refused(w, launcher, WRITES, &WRITE_OPS, &w.at(dir));
    }
    for dir in plants {
        all_refused(w, launcher, PLANTS, &PLANT_OPS, &w.at(dir));
    }
    assert_eq!(snapshot(&w.at(root)), before, "the other matter changed");
    for dir in plants {
        for name in PLANTED {
            let path = w.at(dir).join(name);
            assert!(
                std::fs::symlink_metadata(&path).is_err(),
                "{}",
                path.display()
            );
        }
    }
    for source in ["m1/in.txt", "m1/plant.md", "m1/plant-dir/settings.json"] {
        assert!(w.at(source).exists(), "{source} moved");
    }
    for moved in ["m1/out.txt", "m1/empty-out"] {
        assert!(
            !w.at(moved).exists(),
            "{moved} came out of the other matter"
        );
    }
}

/// `w`'s policy for `m1` with `root` (relative) hidden as matter `m2-id`, and `extra` applied.
fn hiding(w: &World, root: &str, extra: impl FnOnce(PolicyBuilder) -> PolicyBuilder) -> Policy {
    extra(w.builder().hidden_matter(&w.at(root), "m2-id"))
        .build()
        .unwrap()
}

/// The error `builder` builds to, as text.
fn refusal(builder: PolicyBuilder) -> String {
    builder.build().unwrap_err().to_string()
}

#[test]
fn a_read_grant_of_another_matter_writes_nothing_at_its_root_or_below() {
    let w = World::new();
    other_matter(&w, "m2");
    let policy = hiding(&w, "m2", |b| b.other_matter_read(&w.at("m2"), false));
    let (launcher, _) = launcher(policy);
    assert_read_only(&w, &launcher, "m2", &["m2", "m2/sub", "m2/sub/deeper"], &[]);
    // Nothing the test reads changed: no xattr was written, and `doc.txt` reads as before.
    assert!(!snapshot(&w.at("m2")).1.contains("com.example"));
    assert_eq!(
        std::fs::read_to_string(w.at("m2/sub/doc.txt")).unwrap(),
        "doc\n"
    );
}

#[test]
fn agent_instruction_files_cannot_be_planted_anywhere_in_a_granted_matter() {
    let w = World::new();
    other_matter(&w, "m2");
    let policy = hiding(&w, "m2", |b| b.other_matter_read(&w.at("m2"), false));
    let (launcher, _) = launcher(policy);
    // The root, a folder holding a `.git`, and a folder below it holding a `.git` of its own.
    let plants = ["m2", "m2/sub", "m2/sub/deeper", "m2/empty"];
    assert_read_only(&w, &launcher, "m2", &[], &plants);
}

#[test]
fn a_read_grant_of_a_folder_writes_nothing_in_it_or_beside_it() {
    let w = World::new();
    other_matter(&w, "m2");
    let policy = hiding(&w, "m2", |b| b.other_matter_read(&w.at("m2/sub"), false));
    let (launcher, _) = launcher(policy);
    let (writes, plants) = (
        ["m2/sub", "m2/sub/deeper", "m2"],
        ["m2/sub", "m2/sub/deeper", "m2"],
    );
    assert_read_only(&w, &launcher, "m2", &writes, &plants);
}

#[test]
fn a_read_grant_of_one_file_writes_nothing_to_it_or_beside_it() {
    let w = World::new();
    other_matter(&w, "m2");
    let policy = hiding(&w, "m2", |b| b.other_matter_read(&w.at("m2/doc.txt"), true));
    let (launcher, _) = launcher(policy);
    assert_read_only(&w, &launcher, "m2", &["m2"], &["m2"]);
    // The grant reads: the refusals above are not the file being out of reach.
    let doc = w.at("m2/doc.txt");
    let out = sh(
        &launcher,
        &w.at("m1"),
        r#"cat "$1""#,
        &[doc.to_str().unwrap()],
    );
    assert_eq!(out.assert_ok().stdout, "doc\n");
}

/// The matter, the state folder, the notes folder, the session's temp folder and an allowed
/// folder each refuse to hold a hidden matter, so none can be pointed at one; the credential
/// link must be in the state folder, which holds none.
#[test]
fn no_writable_folder_of_the_policy_may_hold_another_matter() {
    let w = World::new();
    for root in [
        "m1/m3",
        "data/agent-state/m1/codex/m3",
        "data/agent-state/m1/notes/m3",
    ] {
        other_matter(&w, root);
    }
    let grant = |b: PolicyBuilder, root: &str| {
        b.hidden_matter(&w.at(root), "m3-id")
            .other_matter_read(&w.at(root), false)
    };
    let matter = refusal(grant(w.builder(), "m1/m3"));
    assert!(
        matter.contains("the matter may not equal, hold or sit inside a hidden path"),
        "{matter}"
    );
    let state = refusal(grant(w.builder(), "data/agent-state/m1/codex/m3"));
    assert!(
        state.contains("the state folder may not equal or hold a hidden path"),
        "{state}"
    );
    let notes = refusal(grant(w.builder(), "data/agent-state/m1/notes/m3"));
    assert!(
        notes.contains("the notes folder may not equal or hold a hidden path"),
        "{notes}"
    );
    // The session's temp folder, outside the hidden data dir so only its own rule applies.
    other_matter(&w, "run/s1/tmp/m3");
    let tmp = refusal(grant(
        w.builder().tmpdir(w.at("run/s1/tmp")),
        "run/s1/tmp/m3",
    ));
    assert!(
        tmp.contains("the temp folder may not equal or hold a hidden path"),
        "{tmp}"
    );
    // An allowed folder (read only in the profile) holding the other matter, or inside it.
    other_matter(&w, "shared/m3");
    for allowed in ["shared", "shared/m3/sub"] {
        let b = grant(w.builder().allowed_folder(w.at(allowed)), "shared/m3");
        let message = refusal(b);
        assert!(
            message.contains("an allowed folder may not equal, hold or sit inside a hidden path"),
            "{allowed}: {message}"
        );
    }
}

/// The home folder holding the other matter: the profile reads one file of it, so a matter
/// there is as closed to writes as anywhere. The World is under `/private/tmp`, which no rule
/// of the base allows beyond `/tmp`'s own metadata.
#[test]
fn a_read_grant_of_a_matter_in_the_home_folder_writes_nothing() {
    let w = World::new();
    other_matter(&w, "home/m3");
    let policy = hiding(&w, "home/m3", |b| {
        b.other_matter_read(&w.at("home/m3"), false)
    });
    let (launcher, _) = launcher(policy);
    let dirs = ["home/m3", "home/m3/sub", "home/m3/sub/deeper"];
    assert_read_only(&w, &launcher, "home/m3", &dirs, &dirs);
}

/// A runtime path and a read path inside the other matter (allowed again after the hidden
/// paths, read and run only) give no write either.
#[test]
fn runtime_and_read_paths_in_a_granted_matter_write_nothing() {
    let w = World::new();
    other_matter(&w, "m2");
    let policy = hiding(&w, "m2", |b| {
        b.other_matter_read(&w.at("m2"), false)
            .runtime(w.at("m2/sub"))
            .read(w.at("m2/sub/deeper"))
    });
    let (launcher, _) = launcher(policy);
    let dirs = ["m2", "m2/sub", "m2/sub/deeper"];
    assert_read_only(&w, &launcher, "m2", &dirs, &dirs);
}

/// The Claude store pointed at the other matter's root, at a folder in it, or beside it with
/// its lock folder (`<dir>.lock`) on the matter, with and without a read grant: the profile
/// allows its names again after the hidden paths, so the policy refuses to build.
#[test]
fn a_claude_store_on_or_in_another_matter_is_refused_at_build() {
    let w = World::new();
    other_matter(&w, "m2");
    w.dir("store");
    for grant in [false, true] {
        for (store, matter) in [("m2", "m2"), ("m2/sub", "m2"), ("store", "store.lock")] {
            let mut b = w.builder().hidden_matter(&w.at(matter), "m2-id");
            if grant && matter == "m2" {
                b = b.other_matter_read(&w.at("m2"), false);
            }
            let error = b.claude_store(w.at(store)).build().unwrap_err();
            let Error::HiddenOverlap {
                allowance, hidden, ..
            } = &error
            else {
                panic!("store {store}: {error:?}");
            };
            assert_eq!(*allowance, HiddenOverlapAllowance::ClaudeStore);
            assert_eq!(hidden, &w.at(matter), "store {store}");
        }
    }
}

/// The browser socket folder made in the other matter, with and without a read grant: the
/// profile allows writes in it after every other rule, so the policy refuses to build.
#[test]
fn a_browser_socket_folder_in_another_matter_is_refused_at_build() {
    let w = World::new();
    other_matter(&w, "m2");
    let dir = BrowserSocketDir::create_in(&w.at("m2")).unwrap();
    for grant in [false, true] {
        let mut b = w.builder().hidden_matter(&w.at("m2"), "m2-id");
        if grant {
            b = b.other_matter_read(&w.at("m2"), false);
        }
        let error = b.browser_socket_dir(&dir).build().unwrap_err();
        let Error::HiddenOverlap {
            allowance,
            path,
            hidden,
        } = &error
        else {
            panic!("grant {grant}: {error:?}");
        };
        assert_eq!(*allowance, HiddenOverlapAllowance::BrowserSocketDir);
        assert_eq!(path, dir.canonical());
        assert_eq!(hidden, &w.at("m2"));
    }
}
