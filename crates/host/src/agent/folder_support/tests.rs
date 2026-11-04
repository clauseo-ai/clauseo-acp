use std::sync::{Arc, Mutex, mpsc};

use clauseo_protocol::{ErrorCode, FolderSupportKind, MatterId, Timestamp};

use super::{decide, of_refusal};
use crate::ManualClock;
use crate::agent::codex::prepare::{Job, LAUNCH_WAIT, off_actor};

fn matter() -> MatterId {
    MatterId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0001,
    ))
    .unwrap()
}

#[test]
fn a_network_drive_and_a_drive_without_permissions_refuse_codex_and_say_no_path() {
    for (kind, code) in [
        (FolderSupportKind::Network, ErrorCode::MatterFolderNetwork),
        (
            FolderSupportKind::NoPermissions,
            ErrorCode::MatterFolderNoPermissions,
        ),
    ] {
        let error = decide(Some(kind)).unwrap_err();
        assert_eq!(error.code(), code);
        assert!(!code.is_retryable(), "{code}");
        let message = error.to_string();
        assert!(
            !message.contains('\\') && !message.contains('/'),
            "{message}"
        );
        // The row learns the answer from the refusal.
        assert_eq!(of_refusal(code), Some(kind));
    }
}

#[test]
fn a_folder_whose_permissions_cant_change_starts_codex_and_keeps_the_answer() {
    assert_eq!(
        decide(Some(FolderSupportKind::CantChangePermissions)).unwrap(),
        Some(FolderSupportKind::CantChangePermissions)
    );
    assert_eq!(decide(None).unwrap(), None);
    assert_eq!(of_refusal(ErrorCode::MatterCodexFolder), None);
    assert_eq!(of_refusal(ErrorCode::MatterFolderMissing), None);
    assert_eq!(of_refusal(ErrorCode::AgentLaunchFailed), None);
}

/// Off Windows nothing is checked: no answer, even for a folder that is not there.
#[cfg(not(windows))]
#[test]
fn off_windows_there_is_no_check_and_no_answer() {
    let missing = std::env::temp_dir()
        .join("clauseo-folder-support-none")
        .join("x");
    assert_eq!(super::check(matter(), &missing), None);
    assert_eq!(super::check(matter(), &std::env::temp_dir()), None);
    assert_eq!(
        super::refuse_unsupported_folder(matter(), &std::env::temp_dir()).unwrap(),
        None
    );
}

/// On Windows a folder the test made is fine: no answer, and Codex is not refused.
#[cfg(windows)]
#[test]
fn a_folder_the_user_made_has_no_answer() {
    let folder = std::env::temp_dir();
    assert_eq!(super::check(matter(), &folder), None);
    // A missing folder is left to the other folder checks: no answer, no refusal.
    let missing = folder.join("clauseo-folder-support-missing").join("x");
    assert_eq!(
        super::refuse_unsupported_folder(matter(), &missing).unwrap(),
        None
    );
}

/// A folder check that hangs (a share that does not answer) ends as the preparation's deadline
/// ends it, never as `matter.folder_network`, even when the check would have said network.
#[test]
fn a_hung_check_is_never_a_folder_code() {
    let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let blocked = Mutex::new(blocked);
    let hung = off_actor(Arc::new(clock.clone()), Job::launch(matter()), move || {
        let _told = entered.send_blocking(());
        let _released = blocked.lock().map(|blocked| blocked.recv());
        decide(Some(FolderSupportKind::Network))
    });
    let move_on = async move {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, LAUNCH_WAIT);
        clock.advance(LAUNCH_WAIT);
    };
    let (result, ()) = futures::executor::block_on(futures::future::join(hung, move_on));
    let code = result.unwrap_err().code();
    assert_ne!(code, ErrorCode::MatterFolderNetwork);
    assert_ne!(code, ErrorCode::MatterFolderNoPermissions);
    assert_eq!(of_refusal(code), None, "{code}");
    drop(release);
}
