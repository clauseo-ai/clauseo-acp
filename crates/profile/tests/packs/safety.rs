//! Links, paths, the layout, the caps and scripts.

use std::time::Duration;

use clauseo_profile::packs::{
    self, DEPTH_MAX, FILE_MAX, FILES_MAX, FOLDERS_MAX, MANIFEST_MAX, PACK_MAX, PACKS_PER_LAUNCH,
    PATH_MAX, Refusal, Skip,
};

use super::{Scratch, entry, manifest_json, skill_md, support};

#[cfg(unix)]
#[test]
fn a_symlinked_file_or_folder_is_refused() {
    let scratch = Scratch::new("symlink");
    let outside = scratch.join("outside");
    support::write(&outside.join("secret.md"), b"secret\n").unwrap();
    scratch.valid("nda-kit", &["a1"]);
    let refs = scratch.pack("nda-kit").join("skills/a1/references");
    support::symlink(&outside.join("secret.md"), &refs.join("linked.md")).unwrap();
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Link);
    support::remove_file(&refs.join("linked.md")).unwrap();
    support::symlink(&outside, &refs.join("more")).unwrap();
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Link);
    support::remove_file(&refs.join("more")).unwrap();
    assert!(scratch.inspect("nda-kit").is_ok());
    // The pack's own folder as a link.
    support::symlink(&scratch.pack("nda-kit"), &scratch.pack("link-kit")).unwrap();
    assert_eq!(scratch.inspect("link-kit").unwrap_err(), Refusal::Link);
}

/// Windows: a symlinked folder (a reparse point). Making one needs Developer Mode or the
/// privilege; without it the test checks nothing and says so. Junctions and other reparse
/// points take the same attribute check.
#[cfg(windows)]
#[test]
fn a_symlinked_file_or_folder_is_refused() {
    let scratch = Scratch::new("symlink");
    let outside = scratch.join("outside");
    support::write(&outside.join("secret.md"), b"secret\n").unwrap();
    scratch.valid("nda-kit", &["a1"]);
    let link = scratch.pack("nda-kit").join("skills/a1/references/more");
    if support::symlink_dir(&outside, &link).is_err() {
        eprintln!("no symlink privilege: the reparse-point check is not exercised");
        return;
    }
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Link);
}

#[test]
fn a_hard_linked_file_is_refused() {
    let scratch = Scratch::new("hard-link");
    let outside = scratch.join("id_rsa");
    support::write(&outside, b"key\n").unwrap();
    scratch.valid("nda-kit", &["a1"]);
    let linked = scratch.pack("nda-kit").join("skills/a1/references/key.md");
    support::hard_link(&outside, &linked).unwrap();
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::HardLink);
}

#[test]
fn a_path_escaping_the_pack_is_refused() {
    let scratch = Scratch::new("paths");
    // Names Windows cannot hold (or trims) are tried on Unix alone.
    let mut bad = vec![".hidden.md", "a b.md", "x~y.md"];
    if cfg!(unix) {
        bad.extend(["trailing.", "nul.md", "back\\slash.md", "colon:stream.md"]);
    }
    for name in bad {
        scratch.valid("nda-kit", &["a1"]);
        scratch.put("nda-kit", &format!("skills/a1/{name}"), b"x\n");
        assert_eq!(
            scratch.inspect("nda-kit").unwrap_err(),
            Refusal::BadPath,
            "{name}"
        );
        support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    }
    // Two paths that differ only in case (on a disk that keeps both).
    scratch.valid("nda-kit", &["a1"]);
    scratch.put("nda-kit", "skills/a1/Notes.md", b"x\n");
    scratch.put("nda-kit", "skills/a1/notes.md", b"y\n");
    let both = std::fs::read_dir(scratch.pack("nda-kit").join("skills/a1"))
        .unwrap()
        .count()
        == 4;
    if both {
        assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::BadPath);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_name_that_is_not_utf8_is_refused() {
    use std::os::unix::ffi::OsStrExt as _;
    let scratch = Scratch::new("utf8");
    scratch.valid("nda-kit", &["a1"]);
    let name = std::ffi::OsStr::from_bytes(b"bad\xff.md");
    support::write(&scratch.pack("nda-kit").join("skills/a1").join(name), b"x").unwrap();
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::BadPath);
}

#[test]
fn an_unlisted_skill_folder_or_top_level_file_is_refused() {
    let scratch = Scratch::new("unlisted");
    let cases: [(&str, &[u8]); 5] = [
        ("skills/extra/SKILL.md", b"x"),
        ("notes.md", b"x"),
        ("skills/stray.md", b"x"),
        ("docs/README.md", b"x"),
        ("skills/empty-unlisted/.keep-not-hidden.md", b"x"),
    ];
    for (rel, bytes) in cases {
        scratch.valid("nda-kit", &["a1"]);
        scratch.put("nda-kit", rel, bytes);
        let refused = scratch.inspect("nda-kit").unwrap_err();
        assert!(
            matches!(refused, Refusal::Unlisted | Refusal::BadPath),
            "{rel}: {refused:?}"
        );
        support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    }
    // An unlisted folder, even empty.
    scratch.valid("nda-kit", &["a1"]);
    support::make_dir(&scratch.pack("nda-kit").join("skills/ghost")).unwrap();
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Unlisted);
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    // A listed skill with no SKILL.md.
    scratch.valid("nda-kit", &["a1"]);
    scratch.put(
        "nda-kit",
        "clauseo-pack.json",
        manifest_json("nda-kit", &["a1", "b2"], true).as_bytes(),
    );
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Unlisted);
    // Every top-level file the layout allows.
    for rel in [
        "LICENSE",
        "LICENSE.md",
        "LICENSE.txt",
        "NOTICE",
        "README.md",
    ] {
        scratch.valid("nda-kit", &["a1"]);
        scratch.put("nda-kit", rel, b"Licence text.\n");
        assert!(scratch.inspect("nda-kit").is_ok(), "{rel}");
    }
}

/// A pack `nda-kit` of one skill whose files total `total` bytes exactly, padded with
/// `pad-<n>.txt` files of at most [`FILE_MAX`] bytes.
fn pack_of_size(scratch: &Scratch, total: u64) {
    scratch.put(
        "nda-kit",
        "clauseo-pack.json",
        manifest_json("nda-kit", &["a1"], true).as_bytes(),
    );
    let skill = skill_md("a1", "d");
    scratch.put("nda-kit", "skills/a1/SKILL.md", skill.as_bytes());
    let base = manifest_json("nda-kit", &["a1"], true).len() + skill.len();
    let mut left = total - base as u64;
    let mut n = 0;
    while left > 0 {
        let size = left.min(FILE_MAX);
        let bytes = vec![b'x'; usize::try_from(size).unwrap()];
        scratch.put("nda-kit", &format!("skills/a1/pad-{n}.txt"), &bytes);
        left -= size;
        n += 1;
    }
}

#[test]
fn each_size_cap_refuses_one_byte_over() {
    let scratch = Scratch::new("caps");
    let file = |size: u64| vec![b'x'; usize::try_from(size).unwrap()];
    // One file.
    scratch.valid("nda-kit", &["a1"]);
    scratch.put("nda-kit", "skills/a1/big.txt", &file(FILE_MAX));
    assert!(scratch.inspect("nda-kit").is_ok());
    scratch.put("nda-kit", "skills/a1/big.txt", &file(FILE_MAX + 1));
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::TooBig);
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    // The manifest, padded with spaces JSON ignores.
    scratch.valid("nda-kit", &["a1"]);
    let json = manifest_json("nda-kit", &["a1"], true);
    let pad = |to: u64| {
        format!(
            "{json}{}",
            " ".repeat(usize::try_from(to).unwrap() - json.len())
        )
    };
    scratch.put("nda-kit", "clauseo-pack.json", pad(MANIFEST_MAX).as_bytes());
    assert!(scratch.inspect("nda-kit").is_ok());
    scratch.put(
        "nda-kit",
        "clauseo-pack.json",
        pad(MANIFEST_MAX + 1).as_bytes(),
    );
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::TooBig);
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    // The pack's total.
    pack_of_size(&scratch, PACK_MAX);
    assert!(scratch.inspect("nda-kit").is_ok());
    scratch.put("nda-kit", "skills/a1/one-more.txt", b"x");
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::TooBig);
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    // The file count: the manifest, SKILL.md, notes.md, README.md and the rest.
    scratch.valid("nda-kit", &["a1"]);
    for n in 0..FILES_MAX - 4 {
        scratch.put("nda-kit", &format!("skills/a1/f{n}.md"), b"x");
    }
    assert!(scratch.inspect("nda-kit").is_ok());
    scratch.put("nda-kit", "skills/a1/last.md", b"x");
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::TooBig);
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    caps_on_paths(&scratch);
    caps_on_packs(&scratch);
}

/// The depth and path-length caps.
fn caps_on_paths(scratch: &Scratch) {
    // `skills/a1/` then folders, up to DEPTH_MAX slashes.
    let deep = |folders: usize| {
        let folders = (0..folders)
            .map(|n| format!("d{n}"))
            .collect::<Vec<_>>()
            .join("/");
        let folders = if folders.is_empty() {
            folders
        } else {
            format!("/{folders}")
        };
        format!("skills/a1{folders}/x.md")
    };
    scratch.valid("nda-kit", &["a1"]);
    scratch.put("nda-kit", &deep(DEPTH_MAX - 2), b"x");
    assert!(scratch.inspect("nda-kit").is_ok());
    scratch.put("nda-kit", &deep(DEPTH_MAX - 1), b"x");
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::BadPath);
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    let long = |len: usize| format!("skills/a1/{}.md", "n".repeat(len - "skills/a1/.md".len()));
    scratch.valid("nda-kit", &["a1"]);
    scratch.put("nda-kit", &long(PATH_MAX), b"x");
    assert!(scratch.inspect("nda-kit").is_ok());
    scratch.put("nda-kit", &long(PATH_MAX + 1), b"x");
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::BadPath);
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
}

/// The per-launch cap: packs past 16 are skipped as `TooMany`, in the order given. (The
/// 32-skill cap is the manifest's, tested with it.)
fn caps_on_packs(scratch: &Scratch) {
    let mut entries = Vec::new();
    for n in 0..=PACKS_PER_LAUNCH {
        let id = format!("kit-{n}");
        scratch.valid(&id, &["a1"]);
        entries.push(entry(&scratch.inspect(&id).unwrap()));
    }
    let loaded = packs::load(&scratch.packs(), &entries, false);
    assert_eq!(loaded.packs.len(), PACKS_PER_LAUNCH);
    assert_eq!(
        loaded.skipped,
        [(format!("kit-{PACKS_PER_LAUNCH}"), Skip::TooMany)]
    );
}

#[test]
fn scripts_are_refused_without_allow_scripts() {
    let scratch = Scratch::new("scripts");
    let cases: [(&str, &[u8]); 7] = [
        ("run.sh", b"echo hi\n"),
        ("tool.py", b"print(1)\n"),
        ("notes.md", b"#!/bin/sh\necho hi\n"),
        ("image.png", b"\x7fELF\x02\x01"),
        ("image.jpg", b"MZ\x90\x00"),
        ("image.gif", b"\xcf\xfa\xed\xfe"),
        ("macro.docm", b"PK"),
    ];
    for (name, bytes) in cases {
        scratch.valid("nda-kit", &["a1"]);
        scratch.put(
            "nda-kit",
            "clauseo-pack.json",
            manifest_json("nda-kit", &["a1"], false).as_bytes(),
        );
        scratch.put("nda-kit", &format!("skills/a1/{name}"), bytes);
        assert_eq!(
            scratch.inspect("nda-kit").unwrap_err(),
            Refusal::Script,
            "{name}"
        );
        assert!(
            packs::inspect(&scratch.packs(), "nda-kit", true)
                .unwrap()
                .has_scripts(),
            "{name}"
        );
        support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    }
    #[cfg(unix)]
    {
        scratch.valid("nda-kit", &["a1"]);
        scratch.put(
            "nda-kit",
            "clauseo-pack.json",
            manifest_json("nda-kit", &["a1"], false).as_bytes(),
        );
        let notes = scratch
            .pack("nda-kit")
            .join("skills/a1/references/notes.md");
        support::set_mode(&notes, 0o744).unwrap();
        assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Script);
    }
}

#[test]
fn instruction_only_refuses_scripts_even_when_allowed() {
    let scratch = Scratch::new("instruction-only");
    scratch.valid("nda-kit", &["a1"]);
    scratch.put("nda-kit", "skills/a1/run.sh", b"echo hi\n");
    for allow in [false, true] {
        assert_eq!(
            packs::inspect(&scratch.packs(), "nda-kit", allow).unwrap_err(),
            Refusal::ScriptInInstructionOnly
        );
    }
}

#[test]
fn scripts_are_copied_without_the_exec_bit_when_allowed() {
    let scratch = Scratch::new("scripts-copied");
    scratch.valid("nda-kit", &["a1"]);
    scratch.put(
        "nda-kit",
        "clauseo-pack.json",
        manifest_json("nda-kit", &["a1"], false).as_bytes(),
    );
    scratch.put("nda-kit", "skills/a1/run.sh", b"#!/bin/sh\necho hi\n");
    #[cfg(unix)]
    support::set_mode(&scratch.pack("nda-kit").join("skills/a1/run.sh"), 0o755).unwrap();
    let pack = packs::inspect(&scratch.packs(), "nda-kit", true).unwrap();
    assert!(pack.has_scripts());
    let loaded = packs::load(&scratch.packs(), &[entry(&pack)], true);
    assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
    let plugin = scratch.join("run/s1/skill-packs");
    packs::write_packs(&plugin, &loaded.packs).unwrap();
    let copied = plugin.join("skills/nda-kit--a1/run.sh");
    assert_eq!(std::fs::read(&copied).unwrap(), b"#!/bin/sh\necho hi\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&copied).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0, "{} has {mode:o}", copied.display());
    }
}

/// Makes `count` empty folders `f<n>` under `skills/a1/` of `nda-kit`.
fn empty_folders(scratch: &Scratch, count: usize) {
    let skill = scratch.pack("nda-kit").join("skills/a1");
    for n in 0..count {
        support::make_dir(&skill.join(format!("f{n}"))).unwrap();
    }
}

/// D4: folders are counted. `valid` makes 3 (`skills`, `skills/a1`, `skills/a1/references`).
#[test]
fn a_pack_over_64_folders_is_refused() {
    let scratch = Scratch::new("folders");
    scratch.valid("nda-kit", &["a1"]);
    empty_folders(&scratch, FOLDERS_MAX - 3);
    let read = packs::read::read_pack(&scratch.pack("nda-kit")).unwrap();
    assert_eq!(read.folders.len(), FOLDERS_MAX);
    assert!(scratch.inspect("nda-kit").is_ok());
    support::make_dir(&scratch.pack("nda-kit").join("skills/a1/one-more")).unwrap();
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::TooBig);
}

/// The reviewer's case (D4): 1,100 folders, side by side or nested, are refused at once, and the
/// walk holds at most one folder open per depth level, however many folders wait.
#[test]
fn a_thousand_folders_are_refused_quickly_with_one_folder_open_per_level() {
    let scratch = Scratch::new("many-folders");
    scratch.valid("nda-kit", &["a1"]);
    empty_folders(&scratch, 1_100);
    let (refused, took) = support::timed(|| scratch.inspect("nda-kit"));
    assert_eq!(refused.unwrap_err(), Refusal::TooBig);
    assert!(took < Duration::from_secs(1), "{took:?}");
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    scratch.valid("nda-kit", &["a1"]);
    let nested: Vec<&str> = vec!["d"; 1_100];
    let deep = scratch
        .pack("nda-kit")
        .join("skills/a1")
        .join(nested.join("/"));
    support::make_dir(&deep).unwrap();
    let (refused, took) = support::timed(|| scratch.inspect("nda-kit"));
    assert_eq!(refused.unwrap_err(), Refusal::BadPath);
    assert!(took < Duration::from_secs(1), "{took:?}");
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    // 61 siblings waiting: the pack's folder, `skills`, `skills/a1` and the one being listed.
    scratch.valid("nda-kit", &["a1"]);
    empty_folders(&scratch, FOLDERS_MAX - 3);
    let read = packs::read::read_pack(&scratch.pack("nda-kit")).unwrap();
    assert!(read.open_peak <= 4, "{}", read.open_peak);
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    // As deep as a file may sit, with siblings at every level: one open per level.
    scratch.valid("nda-kit", &["a1"]);
    let mut at = scratch.pack("nda-kit").join("skills/a1");
    for level in 0..DEPTH_MAX - 2 {
        for sibling in 0..4 {
            support::make_dir(&at.join(format!("s{sibling}"))).unwrap();
        }
        at = at.join(format!("l{level}"));
    }
    support::write(&at.join("x.md"), b"x").unwrap();
    let read = packs::read::read_pack(&scratch.pack("nda-kit")).unwrap();
    assert!(read.folders.len() > 20, "{}", read.folders.len());
    assert!(read.open_peak <= DEPTH_MAX + 2, "{}", read.open_peak);
    assert!(scratch.inspect("nda-kit").is_ok());
}
