//! Which matter on the list lies inside the session's own: by every spelling, case folded where
//! asked, never by a shared name prefix, never a forgotten one, invited or not.

use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::{held, held_as};
use crate::agent::matter_fence::{MatterFence, Other};
use crate::testing::abs;

fn id(n: u128) -> MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// Matter `n` at `folder`, its one spelling the folder as given.
fn other(n: u128, folder: &str, invited: bool) -> Other {
    let folder = PathBuf::from(abs(folder));
    let mut other = Other::new(id(n), format!("M{n}"), folder.clone(), invited);
    other.spellings = vec![folder];
    other
}

fn own(folder: &str) -> Vec<PathBuf> {
    vec![PathBuf::from(abs(folder))]
}

#[test]
fn a_matter_inside_is_held_invited_or_not_and_a_prefix_or_the_parent_is_not() {
    let others = [
        other(1, "/p/Acme2", false),
        other(2, "/p", false),
        other(3, "/p/Acme", false),
        other(4, "/p/Acme/W/Sub", true),
    ];
    let found = held_as(&others, &own("/p/Acme"), false).map(|other| other.id);
    assert_eq!(found, Some(id(4)), "invited, still held");
    assert_eq!(held_as(&others[..3], &own("/p/Acme"), false), None);
    // The inner matter's conversation: the outer one holds it, it holds nothing.
    assert_eq!(held_as(&others[..3], &own("/p/Acme/W/Sub"), false), None);
}

#[test]
fn a_forgotten_matter_inside_is_not_held() {
    let mut forgotten = other(1, "/p/Acme/W/Sub", false);
    forgotten.listed = false;
    assert_eq!(held_as(&[forgotten], &own("/p/Acme"), false), None);
}

#[test]
fn a_case_variant_spelling_is_held_where_names_fold() {
    let others = [other(1, "/p/ACME/w/Sub", false)];
    assert_eq!(held_as(&others, &own("/p/Acme"), false), None);
    let found = held_as(&others, &own("/p/Acme"), true).map(|other| other.id);
    assert_eq!(found, Some(id(1)));
}

#[test]
fn any_spelling_inside_any_own_spelling_is_held() {
    let mut linked = other(1, "/q/Link/Sub", false);
    linked.spellings.push(PathBuf::from(abs("/p/Acme/W/Sub")));
    let mut fence = MatterFence::asked(vec![linked]);
    fence.own = vec![PathBuf::from(abs("/r/Acme")), PathBuf::from(abs("/p/Acme"))];
    assert_eq!(held(&fence).map(|other| other.id), Some(id(1)));
}

#[cfg(unix)]
#[test]
fn the_recheck_follows_a_link_moved_since_the_launch() {
    use super::recheck;
    use crate::pipeline::tests::support::TempDir;
    use clauseo_protocol::ErrorCode;
    let dir = TempDir::new("held-recheck");
    let root = dunce::canonicalize(dir.path()).unwrap();
    for name in ["Acme/W/Sub", "Elsewhere/Sub"] {
        crate::write::create_dir_all(&root.join(name)).unwrap();
    }
    let link = root.join("Link");
    crate::write::symlink(&root.join("Elsewhere"), &link).unwrap();
    let asked = Other::new(id(1), "Sub".into(), link.join("Sub"), false);
    let fence = MatterFence::asked(vec![asked]);
    let stored = root.join("Acme");
    assert!(recheck(&fence, &stored).is_ok(), "elsewhere");
    crate::write::remove_file(&link).unwrap();
    crate::write::symlink(&root.join("Acme/W"), &link).unwrap();
    let refused = recheck(&fence, &stored).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::MatterHoldsOtherMatter);
    assert_eq!(refused.to_wire().matter(), Some(id(1)));
}

#[test]
fn a_network_or_cloud_folder_is_never_walked() {
    let net = crate::network_paths::NetEnv::current();
    assert!(
        !super::walks(std::path::Path::new(r"\\srv\share\Sub"), &net),
        "share"
    );
    let cloud = PathBuf::from(abs("/Users/a/Library/CloudStorage/OneDrive-Firm/Sub"));
    assert!(
        super::cloud(&cloud) && !super::walks(&cloud, &net),
        "File Provider"
    );
    let icloud = PathBuf::from(abs(
        "/Users/a/Library/Mobile Documents/com~apple~CloudDocs/Sub",
    ));
    assert!(!super::walks(&icloud, &net), "iCloud Drive");
    assert!(
        super::walks(&PathBuf::from(abs("/Users/a/Matters/Sub")), &net),
        "local"
    );
}

/// A link under a cloud folder that leads into the matter is not followed: the re-check decides
/// from its text, so nothing in the cloud folder is looked up.
#[cfg(unix)]
#[test]
fn the_recheck_does_not_follow_a_link_in_a_cloud_folder() {
    use super::recheck;
    use crate::pipeline::tests::support::TempDir;
    let dir = TempDir::new("held-cloud");
    let root = dunce::canonicalize(dir.path()).unwrap();
    let cloud = root.join("Library/CloudStorage/Box");
    for folder in [root.join("Acme/W/Sub"), cloud.clone()] {
        crate::write::create_dir_all(&folder).unwrap();
    }
    crate::write::symlink(&root.join("Acme/W"), &cloud.join("Link")).unwrap();
    let asked = Other::new(id(1), "Sub".into(), cloud.join("Link/Sub"), false);
    assert!(recheck(&MatterFence::asked(vec![asked]), &root.join("Acme")).is_ok());
}

fn manual() -> crate::ManualClock {
    crate::ManualClock::new(clauseo_protocol::Timestamp::new(1_790_000_000_000).unwrap())
}

/// A re-check that does not answer in time fails closed: `agent.launch_failed`, retryable, so
/// the question is kept and the lawyer may try again.
#[test]
fn a_hung_recheck_fails_closed() {
    use std::sync::{Arc, Mutex, mpsc};
    let clock = manual();
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let blocked = Mutex::new(blocked);
    let hung = super::wait(Arc::new(clock.clone()), None, move || {
        let _told = entered.send_blocking(());
        let _released = blocked.lock().map(|blocked| blocked.recv());
        Ok(())
    });
    let move_on = async move {
        started.recv().await.unwrap();
        let wait = clock.next_armed().await;
        clock.advance(wait);
    };
    let (result, ()) = futures::executor::block_on(futures::future::join(hung, move_on));
    let error = result.unwrap_err();
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    assert!(error.code().is_retryable());
    drop(release);
}

/// A re-check whose thread panics fails closed the same way.
#[test]
fn a_panicking_recheck_fails_closed() {
    let failed = super::wait(std::sync::Arc::new(manual()), None, || -> Result<(), _> {
        panic!("the walk failed")
    });
    let error = futures::executor::block_on(failed).unwrap_err();
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
}
