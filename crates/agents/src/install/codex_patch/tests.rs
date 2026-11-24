//! The patch on a copy of codex-acp 2.10.0's modes (`modes-2.10.0.js`, `dist/index.js:43268-43323`
//! verbatim; 2.2.2's `:34264-34319` are the same bytes), on the real file when
//! `CLAUSEO_G16_INSTALL` names an install of the pins, and the pins it is gated on.

use super::*;
use crate::snapshot::Snapshot;
use crate::write::TestDir;

/// codex-acp 2.10.0's `ReadOnly`, `WorkspaceWrite`, `Agent` and `AgentFullAccess` modes.
const MODES_JS: &[u8] = include_bytes!("modes-2.10.0.js");

/// The patched line.
const PATCHED_LINE: &str = "networkAccess: process.env.CLAUSEO_CODEX_NETWORK === \"1\",";
/// The line it replaces.
const OFF_LINE: &str = "networkAccess: false,";

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

/// The text of the mode that `start` begins, up to the next mode.
fn mode<'a>(all: &'a str, start: &str) -> &'a str {
    let at = all.find(start).unwrap() + start.len();
    let end = all[at..].find("static ").map_or(all.len(), |end| at + end);
    &all[at..end]
}

/// `patch` on `text`, which must be refused for `mode`.
fn refused(text: &str, mode: &'static str) {
    assert_eq!(
        patch(text.as_bytes()),
        Err(PatchError::Anchor(mode)),
        "{text}"
    );
}

#[test]
fn the_modes_copy_has_three_network_lines_and_the_patch_names_the_variable() {
    let all = text(MODES_JS);
    assert_eq!(all.matches("networkAccess: false").count(), 3);
    assert_eq!(text(TO), PATCHED_LINE);
    assert!(PATCHED_LINE.contains(VAR));
}

#[test]
fn workspace_write_and_agent_take_the_network_from_the_variable_and_read_only_stays_off() {
    let out = patch(MODES_JS).unwrap().expect("patched");
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
    assert_eq!(all.replace(PATCHED_LINE, OFF_LINE), text(MODES_JS));
}

#[test]
fn the_patch_finds_the_modes_by_their_text_wherever_they_are() {
    let moved = format!("// more code above\nconst x = 1;\n{}", text(MODES_JS));
    let out = patch(moved.as_bytes()).unwrap().expect("patched");
    assert_eq!(text(&out).matches(PATCHED_LINE).count(), 2);
    assert!(text(&out).starts_with("// more code above\n"));
}

#[test]
fn an_already_patched_file_is_left_as_it_is() {
    let out = patch(MODES_JS).unwrap().unwrap();
    assert_eq!(patch(&out).unwrap(), None);
    // One mode patched by hand: only the other is replaced.
    let half = text(MODES_JS).replacen(OFF_LINE, PATCHED_LINE, 1);
    assert_eq!(patch(half.as_bytes()).unwrap().unwrap(), out);
}

#[test]
fn a_mode_missing_or_twice_is_refused() {
    let all = text(MODES_JS);
    refused(
        &all.replacen("static Agent = new", "static Agen = new", 1),
        "agent",
    );
    refused(&format!("{all}{all}"), "workspace-write");
    refused("something else\n", "workspace-write");
}

#[test]
fn a_mode_without_its_line_or_policy_or_with_two_is_refused() {
    let all = text(MODES_JS);
    let agent = all.find("static Agent = new").unwrap();
    let (before, after) = all.split_at(agent);
    // The agent mode's line gone (codex-acp took the setting, or spelt it otherwise).
    refused(
        &format!(
            "{before}{}",
            after.replacen(OFF_LINE, "networkAccess: true,", 1)
        ),
        "agent",
    );
    // Its policy another one.
    refused(
        &format!(
            "{before}{}",
            after.replacen("type: \"workspaceWrite\"", "type: \"other\"", 1)
        ),
        "agent",
    );
    // Two lines in it.
    refused(
        &format!(
            "{before}{}",
            after.replacen(
                OFF_LINE,
                "networkAccess: false,\n      networkAccess: false,",
                1
            )
        ),
        "agent",
    );
    // Both the old line and the patched one.
    let both = format!("{OFF_LINE}\n      {PATCHED_LINE}");
    refused(
        &format!("{before}{}", after.replacen(OFF_LINE, &both, 1)),
        "agent",
    );
    // The error names the version the patch was checked against.
    let err = patch(b"x").unwrap_err();
    assert!(err.to_string().contains("Clauseo patches 2.10.0"), "{err}");
}

#[test]
fn the_file_is_replaced_whole_and_an_unexpected_one_is_untouched() {
    let dir = TestDir::new("codex-patch");
    let path = dir.file("dist/index.js", MODES_JS);
    #[cfg(unix)]
    crate::write::set_mode(&path, 0o755).unwrap();
    assert_eq!(patch_file(&path).unwrap(), Outcome::Patched);
    let out = std::fs::read(&path).unwrap();
    assert_eq!(text(&out).matches(PATCHED_LINE).count(), 2);
    // The tarball's mode (0755) is kept.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "{mode:o}");
    }
    assert_eq!(patch_file(&path).unwrap(), Outcome::AlreadyPatched);
    assert_eq!(std::fs::read(&path).unwrap(), out);
    // No temp file is left beside it.
    let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["index.js"]);

    let other = dir.file("other/index.js", b"something else\n");
    assert!(matches!(
        patch_file(&other),
        Err(FileError::Patch(PatchError::Anchor(_)))
    ));
    assert_eq!(std::fs::read(&other).unwrap(), b"something else\n");
}

/// The pinned codex-acp is the version the patch was checked against, and only Codex's install
/// is patched. A bump fails here until the patch is checked against it, or removed.
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

/// codex-acp 2.10.0's real `dist/index.js`, from an install of the pins (`npm ci` of
/// `crates/limits/tests/live`, as the live suite's `CLAUSEO_G16_INSTALL`): its two modes
/// patched, read-only's line and everything else as it was. Run with `--run-ignored all` and
/// the variable set.
#[test]
#[ignore = "needs CLAUSEO_G16_INSTALL: an npm ci of the pins"]
fn the_real_file_patches_its_two_modes() {
    let install = std::env::var_os("CLAUSEO_G16_INSTALL").expect("CLAUSEO_G16_INSTALL");
    let bytes = std::fs::read(rel(Path::new(&install), FILE)).unwrap();
    let out = patch(&bytes).unwrap().expect("patched");
    let all = text(&out);
    assert_eq!(all.matches(PATCHED_LINE).count(), 2);
    assert_eq!(all.matches("networkAccess: false\n").count(), 1);
    assert_eq!(all.replace(PATCHED_LINE, OFF_LINE).as_bytes(), bytes);
    assert_eq!(patch(&out).unwrap(), None);
}
