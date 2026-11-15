//! A matter inside this one in Claude's sandbox: its entries re-opened one by one as patterns,
//! a name the agent made never turning the sandbox off, and no sandbox never the fallback.

use std::path::{Path, PathBuf};

use super::super::sandbox::{nameable, reopen_pattern};
use super::{Session, mac};

/// A matter inside this one (`Sub`, then deeper down): the matter is not re-opened whole for
/// reading, since Claude Code's `allowRead` beats `denyRead`; its entries down to the other
/// matter are, each as a pattern the runtime never resolves through a link
/// (`reopen_pattern`), and the other matter is denied writes.
#[test]
fn a_matter_inside_this_one_stays_shut_to_sandboxed_commands() {
    for (inner, open) in [
        ("Sub", vec!["Drafts", "lease.pdf"]),
        ("p/q/Sub", vec!["Drafts", "p/other", "p/q/x.docx"]),
    ] {
        let mut session = Session::new("s-1");
        let sub = session.matter.join(inner);
        session.other_matters.push(sub.clone());
        session.matter_inside = vec![sub.clone()];
        session.matter_open = Some(open.iter().map(|name| session.matter.join(name)).collect());
        let sandbox = mac(&session.paths()).unwrap();
        let matter = session.matter.to_str().unwrap();
        let sub = sub.to_str().unwrap();
        assert!(
            !sandbox.allow_read.iter().any(|path| path == matter),
            "{inner}"
        );
        assert!(
            !sandbox.allow_read.iter().any(|path| path.starts_with(sub)),
            "{inner}"
        );
        for name in &open {
            let [itself, below] = reopen_pattern(&session.matter.join(name)).unwrap();
            assert!(sandbox.allow_read.contains(&itself), "{inner}: {itself}");
            assert!(sandbox.allow_read.contains(&below), "{inner}: {below}");
        }
        assert!(sandbox.deny_read.iter().any(|path| path == sub), "{inner}");
        assert!(sandbox.deny_write.iter().any(|path| path == sub), "{inner}");
        // Writes into the rest of the matter still go.
        assert!(
            sandbox.allow_write.iter().any(|path| path == matter),
            "{inner}"
        );
    }
}

/// An entry is written as a one-letter class, `[` escaped, itself and all under it; a name the
/// agent made with `*` or `?`, or with no letter or digit, is never re-opened.
#[test]
fn a_re_opened_entry_is_a_pattern_and_a_wildcard_name_is_left_out() {
    let matter = Session::new("s-1").matter;
    let [itself, below] = reopen_pattern(&matter.join("docs")).unwrap();
    assert!(itself.ends_with("/[d]ocs"), "{itself}");
    assert_eq!(below, format!("{itself}/**"));
    let [bracket, _] = reopen_pattern(&matter.join("_x[1]")).unwrap();
    assert!(bracket.ends_with("/_[x][[]1]"), "{bracket}");
    for name in ["what?.txt", "a*b", "---"] {
        assert_eq!(reopen_pattern(&matter.join(name)), None, "{name}");
    }
    // Such names never turn the sandbox off.
    let mut session = Session::new("s-1");
    let sub = session.matter.join("Sub");
    session.other_matters.push(sub.clone());
    session.matter_inside = vec![sub];
    session.matter_open = Some(vec![
        session.matter.join("what?.txt"),
        session.matter.join("a"),
    ]);
    let sandbox = mac(&session.paths()).expect("the sandbox stays on");
    assert!(!sandbox.allow_read.iter().any(|path| path.contains("what")));
}

/// Where the own matter's entries could not be listed safely, the sandbox stays on and
/// re-opens none of them: never the matter whole, never no sandbox.
#[test]
fn an_unlisted_matter_inside_this_one_keeps_the_sandbox_and_opens_none() {
    let mut session = Session::new("s-1");
    let sub = session.matter.join("Sub");
    session.other_matters.push(sub.clone());
    session.matter_inside = vec![sub];
    session.matter_open = Some(Vec::new());
    let sandbox = mac(&session.paths()).expect("the sandbox stays on");
    let matter = session.matter.to_str().unwrap();
    assert!(
        !sandbox
            .allow_read
            .iter()
            .any(|path| path.starts_with(matter))
    );
}

/// Another matter's folder as registered always goes in the lists; where it only leads (a link
/// the agent made) only when the sandbox can name it, so a link to `*` turns nothing off.
#[test]
fn a_folder_the_agent_can_name_through_a_link_never_turns_the_sandbox_off() {
    let stored = Session::new("s-1").matter.join("Sub");
    let stored = [stored.as_path()];
    assert!(nameable(stored[0], &stored));
    let wild = PathBuf::from("/tmp/a*b");
    assert!(!nameable(&wild, &stored));
    assert!(nameable(Path::new("/tmp/plain"), &stored));
}
