//! A matter inside this one in Claude's sandbox: a fenced one shuts the matter to sandboxed
//! commands, an invited one leaves it open but denied writes, and a deny is never dropped.

use std::path::PathBuf;

use super::super::sandbox::deny_form;
use super::{Session, mac};

/// A fenced matter inside this one (`Sub`, then deeper down): the matter is not re-opened for
/// reading at all, since Claude Code's `allowRead` beats `denyRead`; the other matter is
/// denied reads and writes, and writes into the rest of the matter still go.
#[test]
fn a_fenced_matter_inside_this_one_shuts_it_to_sandboxed_commands() {
    for inner in ["Sub", "p/q/Sub"] {
        let mut session = Session::new("s-1");
        let sub = session.matter.join(inner);
        session.other_matters.push(sub.clone());
        session.matter_inside = vec![sub.clone()];
        session.matter_shut = true;
        let sandbox = mac(&session.paths()).expect("the sandbox stays on");
        let matter = session.matter.to_str().unwrap();
        let sub = sub.to_str().unwrap();
        assert!(
            !sandbox
                .allow_read
                .iter()
                .any(|path| path.starts_with(matter)),
            "{inner}"
        );
        assert!(sandbox.deny_read.iter().any(|path| path == sub), "{inner}");
        assert!(sandbox.deny_write.iter().any(|path| path == sub), "{inner}");
        assert!(
            sandbox.allow_write.iter().any(|path| path == matter),
            "{inner}"
        );
    }
}

/// An invited matter inside this one: the matter is re-opened whole, the invited one in it
/// readable, and still denied writes.
#[test]
fn an_invited_matter_inside_this_one_is_read_whole_and_denied_writes() {
    let mut session = Session::new("s-1");
    let shared = session.matter.join("Shared");
    session.matter_inside = vec![shared.clone()];
    let sandbox = mac(&session.paths()).expect("the sandbox stays on");
    let matter = session.matter.to_str().unwrap();
    let shared = shared.to_str().unwrap();
    assert!(sandbox.allow_read.iter().any(|path| path == matter));
    assert!(!sandbox.deny_read.iter().any(|path| path == shared));
    assert!(sandbox.deny_write.iter().any(|path| path == shared));
}

/// A folder as registered is denied as itself; where it only leads, as itself when the sandbox
/// can write it, else as the longest folder above it that it can (limited), never dropped.
#[test]
fn a_deny_the_sandbox_cannot_write_is_widened_never_dropped() {
    let abs = |path: &str| PathBuf::from(crate::testing::abs(path));
    let stored = abs("/Users/l/Clients/A/W*/Sub");
    let as_stored = [stored.as_path()];
    assert_eq!(deny_form(&stored, &as_stored), (stored.clone(), false));
    let plain = abs("/tmp/plain/Sub");
    assert_eq!(deny_form(&plain, &as_stored), (plain.clone(), false));
    let wild = abs("/Users/l/Clients/A/W?/x*/Sub");
    assert_eq!(deny_form(&wild, &[]), (abs("/Users/l/Clients/A"), true));
}

/// The agent renames a folder above another matter to a wildcard name and links the old name
/// to it (`mv Acme/W "Acme/W*" && ln -s "W*" Acme/W`): the matter's walked spelling
/// (`Acme/W*/Sub`) is denied as `Acme`, the session limited, and the stored one kept.
#[cfg(unix)]
#[test]
fn a_wildcard_rename_behind_a_link_widens_the_deny() {
    use std::path::Path;

    use crate::agent::matter_fence::{MatterFence, Other};
    use crate::pipeline::tests::support::TempDir;
    use crate::write;

    let dir = TempDir::new("sandbox-wild-rename");
    let root = dunce::canonicalize(dir.path()).unwrap();
    for name in ["own", "data", "Acme/W/Sub"] {
        write::create_dir_all(&root.join(name)).unwrap();
    }
    let acme = root.join("Acme");
    write::rename(&acme.join("W"), &acme.join("W*")).unwrap();
    write::symlink(Path::new("W*"), &acme.join("W")).unwrap();
    let stored = acme.join("W/Sub");
    let id = clauseo_protocol::MatterId::new(uuid::Uuid::from_u128(
        (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | 1,
    ))
    .unwrap();
    let (own, data) = (root.join("own"), root.join("data"));
    let fence = MatterFence::asked(vec![Other::new(id, "M1".into(), stored.clone(), false)])
        .resolve(&[own.as_path()], &[data.as_path()]);
    let walked = acme.join("W*/Sub");
    assert!(fence.others[0].spellings.contains(&walked));
    let as_stored = [stored.as_path()];
    let denied: Vec<(PathBuf, bool)> = fence
        .fenced_folders()
        .map(|path| deny_form(path, &as_stored))
        .collect();
    assert!(denied.contains(&(stored.clone(), false)), "{denied:?}");
    assert!(denied.contains(&(acme, true)), "{denied:?}");
}
