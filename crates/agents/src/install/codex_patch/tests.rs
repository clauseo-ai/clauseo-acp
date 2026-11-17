//! The patch on a copy of codex-acp 2.2.2's modes (`modes-2.2.2.js`, `dist/index.js:34264-34319`
//! verbatim), on the real file when `CLAUSEO_G16_INSTALL` names an install of the pins, and the
//! pins it is gated on.

use super::*;
use crate::snapshot::Snapshot;
use crate::write::TestDir;

/// codex-acp 2.2.2's `ReadOnly`, `WorkspaceWrite`, `Agent` and `AgentFullAccess` modes.
const MODES_JS: &[u8] = include_bytes!("modes-2.2.2.js");
/// Its SHA-256, and that of the copy patched.
const MODES_PINS: Pins<'static> = Pins {
    pristine: "f50d4657b61cf52f8ace6a6c96b75ce5491b4b66fcee10b0754a36b6a1a24905",
    patched: "081d767c5f2ad009f26ed777fa27190f260f2606d4498bd318862788e366e939",
};

/// The patched line.
const PATCHED_LINE: &str = "networkAccess: process.env.CLAUSEO_CODEX_NETWORK === \"1\",";

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

/// The text of the mode that `start` begins, up to the next mode.
fn mode<'a>(all: &'a str, start: &str) -> &'a str {
    let at = all.find(start).unwrap() + start.len();
    let end = all[at..].find("static ").map_or(all.len(), |end| at + end);
    &all[at..end]
}

#[test]
fn the_modes_copy_is_codex_acp_2_2_2s() {
    assert_eq!(sha256(MODES_JS), MODES_PINS.pristine);
    assert_eq!(text(TO), PATCHED_LINE);
    assert!(PATCHED_LINE.contains(VAR));
}

#[test]
fn workspace_write_and_agent_take_the_network_from_the_variable_and_read_only_stays_off() {
    let out = patch(MODES_JS, MODES_PINS).unwrap().expect("patched");
    assert_eq!(sha256(&out), MODES_PINS.patched);
    let all = text(&out);
    for start in ["static WorkspaceWrite = new", "static Agent = new"] {
        let body = mode(all, start);
        assert!(body.contains(PATCHED_LINE), "{start}");
        assert!(!body.contains("networkAccess: false"), "{start}");
    }
    let read_only = mode(all, "static ReadOnly = new");
    assert!(read_only.contains("networkAccess: false\n"));
    assert!(!read_only.contains("CLAUSEO_CODEX_NETWORK"));
    assert_eq!(all.matches("CLAUSEO_CODEX_NETWORK").count(), 2);
    // Nothing else moved: the patched copy less the two replacements is the original.
    assert_eq!(
        all.replace(PATCHED_LINE, "networkAccess: false,"),
        text(MODES_JS)
    );
}

#[test]
fn an_unexpected_file_is_refused_and_names_its_hash() {
    let mut other = MODES_JS.to_vec();
    other.extend_from_slice(b"// one more line\n");
    let err = patch(&other, MODES_PINS).unwrap_err();
    assert_eq!(err, PatchError::Unexpected(sha256(&other)));
    assert!(err.to_string().contains("not the 2.2.2 file"), "{err}");
    // codex-acp 2.2.2's real pins refuse the copy: it is not the whole file.
    assert!(matches!(
        patch(MODES_JS, PINS),
        Err(PatchError::Unexpected(_))
    ));
}

#[test]
fn an_already_patched_file_is_left_as_it_is() {
    let out = patch(MODES_JS, MODES_PINS).unwrap().unwrap();
    assert_eq!(patch(&out, MODES_PINS).unwrap(), None);
}

#[test]
fn a_result_other_than_the_pin_is_refused() {
    let pins = Pins {
        pristine: MODES_PINS.pristine,
        patched: PRISTINE,
    };
    let err = patch(MODES_JS, pins).unwrap_err();
    assert_eq!(err, PatchError::Result(MODES_PINS.patched.to_owned()));
}

#[test]
fn a_mode_without_its_line_is_refused() {
    let gone = text(MODES_JS).replacen("static Agent = new", "static Agen = new", 1);
    let pins = Pins {
        pristine: &sha256(gone.as_bytes()),
        patched: MODES_PINS.patched,
    };
    assert_eq!(
        patch(gone.as_bytes(), pins),
        Err(PatchError::Anchor("agent"))
    );
    // Twice is refused too.
    let twice = format!("{}{}", text(MODES_JS), text(MODES_JS));
    let pins = Pins {
        pristine: &sha256(twice.as_bytes()),
        patched: MODES_PINS.patched,
    };
    assert_eq!(
        patch(twice.as_bytes(), pins),
        Err(PatchError::Anchor("workspace-write"))
    );
}

#[test]
fn the_file_is_replaced_whole_and_an_unexpected_one_is_untouched() {
    let dir = TestDir::new("codex-patch");
    let path = dir.file("dist/index.js", MODES_JS);
    #[cfg(unix)]
    crate::write::set_mode(&path, 0o755).unwrap();
    assert_eq!(patch_file(&path, MODES_PINS).unwrap(), Outcome::Patched);
    let out = std::fs::read(&path).unwrap();
    assert_eq!(sha256(&out), MODES_PINS.patched);
    // The tarball's mode (0755) is kept.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "{mode:o}");
    }
    assert_eq!(
        patch_file(&path, MODES_PINS).unwrap(),
        Outcome::AlreadyPatched
    );
    // No temp file is left beside it.
    let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["index.js"]);

    let other = dir.file("other/index.js", b"something else\n");
    assert!(matches!(
        patch_file(&other, MODES_PINS),
        Err(FileError::Patch(PatchError::Unexpected(_)))
    ));
    assert_eq!(std::fs::read(&other).unwrap(), b"something else\n");
}

/// The pinned codex-acp is the version the patch is made for, and only Codex's install is
/// patched. A bump fails here until the patch is redone for it, or removed.
#[test]
fn pinned_codex_acp_is_the_patched_version() {
    let snapshot = Snapshot::embedded();
    let codex = AgentId::new("codex-acp").unwrap();
    let claude = AgentId::new("claude-acp").unwrap();
    let codex_pins = snapshot.pins(&codex).unwrap();
    assert_eq!(codex_pins.version(), VERSION);
    assert!(patched_by(codex_pins));
    assert!(!patched_by(snapshot.pins(&claude).unwrap()));
}

/// codex-acp 2.2.2's real `dist/index.js`, from an install of the pins (`npm ci` of
/// `crates/limits/tests/live`, as the live suite's `CLAUSEO_G16_INSTALL`): pristine, and
/// patched to [`PATCHED`]. Run with `--run-ignored all` and the variable set.
#[test]
#[ignore = "needs CLAUSEO_G16_INSTALL: an npm ci of the pins"]
fn the_real_2_2_2_file_patches_to_the_pinned_hash() {
    let install = std::env::var_os("CLAUSEO_G16_INSTALL").expect("CLAUSEO_G16_INSTALL");
    let bytes = std::fs::read(rel(Path::new(&install), FILE)).unwrap();
    assert_eq!(sha256(&bytes), PRISTINE);
    let out = patch(&bytes, PINS).unwrap().expect("patched");
    assert_eq!(sha256(&out), PATCHED);
    assert_eq!(patch(&out, PINS).unwrap(), None);
}
