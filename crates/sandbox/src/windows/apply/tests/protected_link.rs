//! S15 `PR4a2`: a protected name in the matter that is a link refuses the launch with its own
//! error, naming the item relative to the matter; a change since the check stays `Changed`.

use super::*;
use clauseo_protocol::ErrorCode;

#[cfg(unix)]
#[test]
fn a_protected_name_that_is_a_link_at_depth_is_named_relative_to_the_matter() {
    let w = world("apply-protected-link", &[]);
    w.dir.file("matter/sub/notes.md", "# the lawyer's\n");
    w.dir.dir("matter/sub/deep");
    w.dir
        .symlink("matter/sub/deep/AGENTS.md", Path::new("../notes.md"));
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = launch_plan(&w, &[]);
    let error = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap_err();
    let Error::ProtectedLink { at } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(at, Path::new("sub/deep/AGENTS.md"));
    assert_eq!(error.code(), ErrorCode::MatterProtectedLink);
    assert_eq!(
        error.to_string(),
        "sub/deep/AGENTS.md is a shortcut, and Clauseo can't protect a shortcut. Move or delete \
         it, then open the matter again."
    );
    let root = w.canonical("matter");
    assert!(!error.to_string().contains(&*root.to_string_lossy()));
    // Rolled back, as any refused launch is.
    assert_eq!(ledger.entries().count(), 0);
    assert!(acl.aces.is_empty(), "{:?}", acl.aces);
}

#[test]
fn a_protected_name_changed_since_the_check_stays_changed() {
    let w = world("apply-protected-changed", &[]);
    w.dir.file("matter/sub/CLAUDE.md", "# the lawyer's\n");
    let claude = w.canonical("matter/sub/CLAUDE.md");
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.fail_add_with = Some((claude, ApplyFailure::Changed));
    let mut plan = launch_plan(&w, &[]);
    let error = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap_err();
    assert!(
        matches!(
            error,
            Error::AclApply {
                path_kind: PathKind::ReadOnlyInMatter,
                failure: ApplyFailure::Changed,
                at: None,
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
}
