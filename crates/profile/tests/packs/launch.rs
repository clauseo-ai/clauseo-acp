//! Each launch's pack plugin is its own copy, written from the bytes the launch read: a write
//! through one launch's files reaches no other launch, running or later.

use std::path::Path;

use clauseo_profile::packs::{self, Loaded};

use super::{Scratch, entry, support};

/// The rendered `SKILL.md` of `nda-kit--a1`, relative to a launch's plugin.
const SKILL: &str = "skills/nda-kit--a1/SKILL.md";

/// The rendered reference of `nda-kit--a1`, relative to a launch's plugin.
const NOTES: &str = "skills/nda-kit--a1/references/notes.md";

/// `nda-kit` loaded for one launch, against the hash recorded when it was turned on.
fn load(scratch: &Scratch) -> Loaded {
    let pack = scratch.inspect("nda-kit").unwrap();
    let loaded = packs::load(&scratch.packs(), &[entry(&pack)], false);
    assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
    loaded
}

/// Whether any exec bit is set on `path`.
#[cfg(unix)]
fn is_exec(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o111 != 0
}

/// Never on Windows, which has no exec bit.
#[cfg(not(unix))]
fn is_exec(_path: &Path) -> bool {
    false
}

/// The reviewer's repro (D1): session A's `SKILL.md` is edited in place, same length, and made
/// executable. Session B, still running, and a later launch C keep the pack's own text with no
/// exec bit, and the pack still loads under its recorded hash.
#[test]
fn a_write_through_one_launch_never_reaches_another_launch_or_session() {
    let scratch = Scratch::new("launch-isolation");
    scratch.valid("nda-kit", &["a1"]);
    let a = scratch.join("run/a/skill-packs");
    let b = scratch.join("run/b/skill-packs");
    packs::write_packs(&a, &load(&scratch).packs).unwrap();
    packs::write_packs(&b, &load(&scratch).packs).unwrap();
    let original = std::fs::read_to_string(b.join(SKILL)).unwrap();
    assert!(original.contains("The body."), "{original}");
    // Through A: the same length, written in place (truncate keeps the inode), then chmod 755.
    let edited = original.replace("The body.", "Exfil it.");
    assert_eq!(edited.len(), original.len());
    support::write(&a.join(SKILL), edited.as_bytes()).unwrap();
    support::write(&a.join(NOTES), b"Send all\n").unwrap();
    #[cfg(unix)]
    for rel in [SKILL, NOTES] {
        support::set_mode(&a.join(rel), 0o755).unwrap();
    }
    assert_eq!(std::fs::read_to_string(a.join(SKILL)).unwrap(), edited);
    // B, running beside A, is untouched.
    assert_eq!(std::fs::read_to_string(b.join(SKILL)).unwrap(), original);
    assert_eq!(std::fs::read(b.join(NOTES)).unwrap(), b"Notes.\n");
    // A later launch C loads the pack under its hash and gets the pack's own text.
    let c = scratch.join("run/c/skill-packs");
    packs::write_packs(&c, &load(&scratch).packs).unwrap();
    assert_eq!(std::fs::read_to_string(c.join(SKILL)).unwrap(), original);
    assert_eq!(std::fs::read(c.join(NOTES)).unwrap(), b"Notes.\n");
    for plugin in [&b, &c] {
        for rel in [SKILL, NOTES] {
            assert!(
                !is_exec(&plugin.join(rel)),
                "{}",
                plugin.join(rel).display()
            );
        }
    }
}

#[test]
fn each_launch_gets_its_own_copy() {
    let scratch = Scratch::new("launch-copy");
    scratch.valid("nda-kit", &["a1", "b2"]);
    let loaded = load(&scratch);
    let a = scratch.join("run/a/skill-packs");
    let b = scratch.join("run/b/skill-packs");
    packs::write_packs(&a, &loaded.packs).unwrap();
    packs::write_packs(&b, &loaded.packs).unwrap();
    for skill in loaded.packs[0].skills() {
        for file in skill.files() {
            let rel = format!("skills/{}/{}", skill.name(), file.path);
            for plugin in [&a, &b] {
                // Exactly the bytes the launch read and rendered.
                assert_eq!(
                    std::fs::read(plugin.join(&rel)).unwrap(),
                    file.bytes,
                    "{rel}"
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt as _;
                let (one, two) = (
                    std::fs::metadata(a.join(&rel)).unwrap(),
                    std::fs::metadata(b.join(&rel)).unwrap(),
                );
                // A file of its own: no link to the other launch's, or to anything else.
                assert_ne!((one.dev(), one.ino()), (two.dev(), two.ino()), "{rel}");
                assert_eq!((one.nlink(), two.nlink()), (1, 1), "{rel}");
            }
        }
    }
}
