//! Another matter's registered folder holding `*` or `?` in Claude's sandbox: the pattern it is
//! on macOS, widened on Linux, and no sandbox when that deny would cover the session's own
//! folders (`agent/loads/sandbox/glob.rs`).
// unix-paths-ok: sandbox paths as text, matched and compared, never opened

use std::path::{Path, PathBuf};

use super::super::sandbox::glob::deny_covers;
use super::super::sandbox::{ClaudeSandbox, Off, OtherMatter, SandboxOs};
use super::{Session, mac};

/// The session's own folders: what its lists allow, before escaping.
fn own(session: &Session) -> Vec<PathBuf> {
    let mut own = vec![
        session.matter.clone(),
        session.work.clone(),
        session.tmp.clone(),
        session.notes.clone(),
        session.downloads.clone(),
        session.skill_packs.clone(),
        session.env_file.clone(),
    ];
    own.extend(session.claude_tmp.clone());
    own.extend(session.skills.iter().cloned());
    own
}

/// A neighbour named `What?` or `A*B` keeps the sandbox on: its deny is the pattern, which
/// covers the folder itself and none of the session's own.
#[test]
fn a_wildcard_neighbour_is_denied_as_its_pattern() {
    for name in ["What?", "A*B"] {
        let mut session = Session::new("s-1");
        let other = PathBuf::from("/Users/l/Clients").join(name);
        session.other_matters.push(OtherMatter::at(other.clone()));
        let sandbox = mac(&session.paths()).expect("the sandbox stays on");
        let pattern = other.to_str().unwrap();
        assert!(
            sandbox.deny_read.iter().any(|deny| deny == pattern),
            "{name}"
        );
        assert!(deny_covers(pattern, pattern), "{name}");
        for path in own(&session) {
            assert!(
                !deny_covers(pattern, path.to_str().unwrap()),
                "{name}: {path:?}"
            );
        }
    }
}

/// Stage C's names, both listed here: `Test*` and `stagec-proof-claude-[x]`. Both keep the
/// sandbox on; the `[` is escaped, so each deny covers the folder it names.
/// Not on Windows, which has no such sandbox: its paths join with `\`.
#[test]
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn stage_cs_registered_names_keep_the_sandbox_on() {
    let docs = Path::new("/Users/rohan/Documents");
    let star = docs.join("stagec-1097/Test*");
    let bracket = docs.join("stagec-proof-claude-[x]");
    let mut session = Session::new("s-1");
    session.matter = docs.join("stagec-1097/Matter A");
    session.other_matters = vec![
        OtherMatter::at(star.clone()),
        OtherMatter::at(bracket.clone()),
    ];
    let sandbox = mac(&session.paths()).expect("the sandbox stays on");
    let star = star.to_str().unwrap();
    assert!(sandbox.deny_read.iter().any(|deny| deny == star));
    let escaped = "/Users/rohan/Documents/stagec-proof-claude-[[]x]";
    assert!(sandbox.deny_read.iter().any(|deny| deny == escaped));
    assert!(deny_covers(star, star));
    assert!(deny_covers(
        bracket.to_str().unwrap(),
        bracket.to_str().unwrap()
    ));
    // A matter `Test*` would also deny: no sandbox.
    session.matter = docs.join("stagec-1097/Testing");
    assert_eq!(mac(&session.paths()), Err(Off::CoversOwn));
}

/// A stored sibling `/x/?` would deny the session's matter `/x/A` itself: no sandbox, on
/// macOS (the pattern matches it) and on Linux (the folder it widens to holds it).
#[test]
fn a_neighbour_pattern_over_the_own_matter_sends_no_sandbox() {
    let mut session = Session::new("s-1");
    session.matter = PathBuf::from("/x/A");
    session.other_matters = vec![OtherMatter::at("/x/?")];
    for os in [SandboxOs::Mac, SandboxOs::Linux] {
        assert_eq!(
            ClaudeSandbox::for_os(os, &session.paths()),
            Err(Off::CoversOwn),
            "{os:?}"
        );
    }
}

/// On Linux a neighbour's pattern is denied as the folder above it with no `*` or `?`; when that
/// folder holds the session's matter, no sandbox.
#[test]
fn linux_widens_a_wildcard_neighbour_or_refuses() {
    let mut session = Session::new("s-1");
    session.other_matters = vec![OtherMatter::at("/Users/l/Elsewhere/What?/2024")];
    let linux = ClaudeSandbox::for_os(SandboxOs::Linux, &session.paths()).unwrap();
    assert!(
        linux
            .deny_read
            .iter()
            .any(|deny| deny == "/Users/l/Elsewhere")
    );
    assert!(!linux.deny_read.iter().any(|deny| deny.contains('?')));
    session.other_matters = vec![OtherMatter::at("/Users/l/Clients/What?")];
    let linux = ClaudeSandbox::for_os(SandboxOs::Linux, &session.paths());
    assert_eq!(linux, Err(Off::CoversOwn));
    assert!(mac(&session.paths()).is_ok());
}

/// The security review of #1153, L1: the "covers own matter" check compares the matter folder
/// where it leads as well as stored, folded as Seatbelt matches (case and Unicode form) on
/// macOS: `/tmp/x/Own` (which leads to `/private/tmp/x/Own`) against `/private/tmp/x/O*`,
/// `/Users/l/clients/Own` against `/Users/l/Clients/O*`, and an NFC name against an NFD
/// pattern each refuse; so does the walked spelling on Linux.
#[test]
fn the_own_matter_is_covered_by_where_it_leads_and_folded() {
    let cases: [(&str, Option<&str>, &str); 3] = [
        (
            "/tmp/x/Own",
            Some("/private/tmp/x/Own"),
            "/private/tmp/x/O*",
        ),
        ("/Users/l/clients/Own", None, "/Users/l/Clients/O*"),
        (
            "/Users/l/Clients/Caf\u{e9}",
            None,
            "/Users/l/Clients/Cafe\u{301}*",
        ),
    ];
    for (matter, walked, other) in cases {
        let mut session = Session::new("s-1");
        session.matter = PathBuf::from(matter);
        session.other_matters = vec![OtherMatter::at(other)];
        assert!(
            mac(&session.paths()).is_ok(),
            "{matter}: as stored alone, no match"
        );
        session.matter_also = walked.into_iter().map(PathBuf::from).collect();
        if walked.is_none() {
            // The fold alone finds it.
            assert_eq!(mac(&session.paths()), Err(Off::CoversOwn), "{matter}");
            continue;
        }
        assert_eq!(mac(&session.paths()), Err(Off::CoversOwn), "{matter}");
        let linux = ClaudeSandbox::for_os(SandboxOs::Linux, &session.paths());
        assert_eq!(linux, Err(Off::CoversOwn), "{matter} on Linux");
    }
}

/// Where another matter's folder leads is left out of the denies when it is `/`, a system
/// folder the shell reads or one above it, a folder above the session's own or the stored
/// folder it came from, compared folded on macOS; a link to `~/Documents` stays denied.
#[test]
fn where_a_link_leads_is_left_open_only_for_the_shells_places() {
    use super::super::sandbox::links::left_open;
    let stored = Path::new("/Users/l/Clients/B");
    let own = [Path::new("/Users/l/Clients/A")];
    let mac_open = |walked: &str| left_open(SandboxOs::Mac, Path::new(walked), stored, &own);
    for walked in [
        "/",
        "/usr",
        "/private",
        "/private/var",
        "/Users",
        "/users/L/clients",
    ] {
        assert!(mac_open(walked), "{walked}");
    }
    for walked in ["/Users/l/Documents", "/usr/local/x", "/Users/l/Clients/C"] {
        assert!(!mac_open(walked), "{walked}");
    }
    let linux = left_open(SandboxOs::Linux, Path::new("/users/l"), stored, &own);
    assert!(!linux, "Linux compares names as they are");
}

/// The own-matter sweep's A15 on macOS: a neighbour registered under the data volume's spelling
/// (`/System/Volumes/Data/Users/…`, a firmlink twin) covers the own matter at `/Users/…`, and
/// the other way round; and a link to the twin of a folder above the own matter is left open.
#[test]
fn a_firmlink_twin_meets_the_own_matter() {
    use super::super::sandbox::links::left_open;
    let pairs = [
        (
            "/Users/l/Clients/Own",
            "/System/Volumes/Data/Users/l/Clients/O*",
        ),
        (
            "/System/Volumes/Data/Users/l/Clients/Own",
            "/Users/l/Clients/O*",
        ),
    ];
    for (matter, other) in pairs {
        let mut session = Session::new("s-1");
        session.matter = PathBuf::from(matter);
        session.other_matters = vec![OtherMatter::at(other)];
        assert_eq!(mac(&session.paths()), Err(Off::CoversOwn), "{matter}");
    }
    let own = [Path::new("/Users/l/Clients/A")];
    let twin = Path::new("/System/Volumes/Data/Users/l");
    assert!(left_open(SandboxOs::Mac, twin, Path::new("/x/B"), &own));
}
