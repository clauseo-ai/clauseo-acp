//! The ruling on a tool-call-only card, branch by branch: credential places never; another
//! matter only when an OS report of the same file confirms it; anything else only as one file
//! the launch would deny; the jail judged on a pinned path.

use std::path::Path;

use clauseo_protocol::{DenialKind, OtherMatter};
use clauseo_sandbox::PathClass;

#[cfg(unix)]
use super::Jail;
use super::{Facts, LaunchPolicy, facts, tool_call_ruling};
#[cfg(unix)]
use crate::actor::turn::denials::offer::identity::tests::{aliased, capture};
use crate::actor::turn::denials::offer::identity::tests::{matter, rules};
use crate::actor::turn::denials::offer::{Offer, Refused, offer};

/// Denies everything, or nothing.
struct Policy(bool);

impl LaunchPolicy for Policy {
    fn would_deny(&self, _path: &Path) -> bool {
        self.0
    }
}

/// What an OS report of `path` gets, and the facts under `policy`.
fn judged(path: &str, is_file: bool, policy: &dyn LaunchPolicy) -> (Offer, Facts) {
    let rules = rules();
    let path = Path::new(path);
    let class = PathClass::Outside;
    let base = offer(
        &rules,
        Refused::Path {
            path,
            is_file,
            class,
            holds_protected: false,
        },
    );
    (base, facts(&rules, path, is_file, class, policy))
}

#[test]
fn a_credential_place_is_never_offered_whatever_confirms_it() {
    for path in [
        "/home/a/.ssh/id_rsa",
        "/home/a/Library/Keychains/login.keychain-db",
    ] {
        let (base, facts) = judged(path, true, &Policy(true));
        assert!(facts.never, "{path}");
        assert!(!tool_call_ruling(base, &facts, true).offered, "{path}");
    }
}

#[test]
fn another_matter_is_offered_only_when_an_os_report_confirms_it() {
    let path = "/home/a/Matters/Smith/brief.docx";
    let (base, facts) = judged(path, true, &Policy(true));
    let read_only = Offer {
        kind: DenialKind::File {
            path: path.into(),
            other_matter: Some(OtherMatter {
                matter: matter(2),
                name: "Smith".into(),
            }),
        },
        offered: true,
    };
    assert_eq!(base, read_only);
    // A tool call's claim alone: a plain line, no grant.
    let claim = tool_call_ruling(base.clone(), &facts, false);
    assert_eq!(
        claim,
        Offer {
            offered: false,
            ..read_only.clone()
        }
    );
    // Confirmed by the OS: the read-only card an OS report gets.
    assert_eq!(tool_call_ruling(base, &facts, true), read_only);
}

#[test]
fn anything_else_is_one_file_the_launch_would_deny() {
    let path = "/home/a/Deals/spa.docx";
    let (base, facts) = judged(path, true, &Policy(true));
    // An OS report of it gets the whole folder; a tool call never does.
    assert_eq!(
        base.kind,
        DenialKind::Folder {
            path: "/home/a/Deals".into(),
            other_matter: None
        }
    );
    let one_file = Offer {
        kind: DenialKind::File {
            path: path.into(),
            other_matter: None,
        },
        offered: true,
    };
    assert_eq!(tool_call_ruling(base, &facts, false), one_file);
    // Not what the launch would deny: nothing offered.
    let (base, facts) = judged(path, true, &Policy(false));
    assert!(!tool_call_ruling(base, &facts, true).offered);
    // A folder: never offered from a tool call.
    let (base, facts) = judged("/home/a/Deals", false, &Policy(true));
    assert!(base.offered);
    assert!(!tool_call_ruling(base, &facts, true).offered);
}

// Unix fixture paths aren't absolute on Windows, where the walk refuses them; Windows' walk
// rules are tested in network_paths' Windows tests.
#[cfg(unix)]
#[test]
fn the_jail_judges_a_pinned_path_so_a_respelled_matter_is_inside() {
    let ids = aliased();
    let captured = capture(rules(), &ids);
    let jail = Jail(&captured.rules);
    assert!(!jail.would_deny(Path::new("/home/a/Matters/Acme/brief.docx")));
    assert!(!jail.would_deny(Path::new("/home/a/.local/share/clauseo/run/x")));
    assert!(jail.would_deny(Path::new("/home/a/Deals/spa.docx")));
    // `CLAUSE~1` is the data folder: pinned, it is inside.
    let alias = captured
        .places
        .pin(Path::new("/home/a/.local/share/CLAUSE~1/run/x"), &ids)
        .unwrap();
    assert!(!jail.would_deny(&alias));
}
