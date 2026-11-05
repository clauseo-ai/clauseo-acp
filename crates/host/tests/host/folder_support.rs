//! The Windows folder check of a matter folder, through the host: at the create and at each
//! Codex launch its answer reaches the matter's row (`MatterSummary::folder_support`); a network
//! drive refuses Codex with `matter.folder_network`; a folder whose permissions the lawyer can't
//! change starts Codex, and the row keeps saying why its shell's writes there fail. Off Windows
//! nothing is checked and the row never has an answer. The check's own answers are tested in
//! `clauseo_sandbox::windows`; its refusals and its deadline in `agent/folder_support/tests.rs`.
//! `HostQuery::FolderSupport` is in `folder_query.rs`.

use clauseo_protocol::{FolderSupportKind, MatterId};

use crate::matters::summary;
use crate::support::Running;

/// The folder check's answer on `matter`'s row, as `ListMatters` gives it.
fn support_of(host: &Running, matter: MatterId) -> Option<FolderSupportKind> {
    summary(host, matter).folder_support
}

/// Off Windows a Codex launch checks nothing: the row has no answer before or after it.
#[cfg(not(windows))]
#[test]
fn a_codex_launch_off_windows_leaves_no_answer() {
    use clauseo_protocol::{StopReason, StreamId};

    use crate::codex::{CODEX, codex_dir, run_turn};

    let dir = codex_dir("folder_support_none", "codex-handshake");
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    assert_eq!(support_of(&host, matter), None);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_ended(turn).unwrap().stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert_eq!(support_of(&host, matter), None);
    host.shutdown();
}

#[cfg(windows)]
mod windows {
    use std::path::{Component, Path, PathBuf, Prefix};
    use std::sync::Arc;

    use clauseo_protocol::ToolCallStatus;
    use clauseo_protocol::{DomainEvent, ErrorCode, FolderSupportKind, StopReason, StreamId};

    use super::support_of;
    use crate::codex::{CODEX, Recording, codex_dir, run_turn, turn_failed};
    use crate::support::{Dir, Scenario};

    /// A folder the lawyer can't re-permission: `OWNER RIGHTS` with Modify only (no
    /// `WRITE_DAC`), `SYSTEM` with full access, nothing inherited.
    const CANT_CHANGE: &str = "D:P(A;OICI;0x1301bf;;;OW)(A;OICI;FA;;;SY)";

    /// `folder` through the admin share of its drive: `C:\x\y` as `\\localhost\C$\x\y`.
    fn through_share(folder: &Path) -> PathBuf {
        let mut parts = folder.components();
        let Some(Component::Prefix(prefix)) = parts.next() else {
            panic!("{folder:?} has no drive");
        };
        let letter = match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => char::from(letter),
            other => panic!("{folder:?} is not on a drive letter: {other:?}"),
        };
        let mut share = PathBuf::from(format!(r"\\localhost\{letter}$\"));
        share.extend(parts.filter(|part| !matches!(part, Component::RootDir)));
        share
    }

    /// A matter on a network drive, a subfolder of the admin share (never the share's root):
    /// the create already says so on its row, and a Codex turn fails with
    /// `matter.folder_network`, unlaunched. CI's Windows runners have the share, so a missing
    /// one fails there; elsewhere the test says so and passes.
    #[test]
    fn codex_on_a_network_drive_is_refused_unlaunched() {
        let dir = codex_dir("folder_support_network", "codex-handshake");
        let local = dir.folder("x", &[("brief.txt", "x\n")]);
        let folder = through_share(&local);
        if !folder.is_dir() {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI's Windows runners have the admin share, and this run has none: {folder:?}"
            );
            eprintln!("skipped: this machine has no admin share at {folder:?}");
            return;
        }
        let launcher = Recording::new(&dir);
        let envs = Arc::clone(&launcher.envs);
        let host = dir.start_launching(0, Arc::new(launcher), |config| config);
        let matter = host.create_matter("x", &folder);
        assert_eq!(support_of(&host, matter), Some(FolderSupportKind::Network));
        let thread = host.create_thread(matter, CODEX);
        let mut events = host.events(StreamId::Thread(thread));
        let turn = run_turn(&host, &mut events, thread);
        let failed = turn_failed(&events, turn)
            .unwrap_or_else(|| panic!("no TurnFailed of {turn}: {}", events.dump()));
        assert_eq!(
            failed.code,
            ErrorCode::MatterFolderNetwork,
            "{}",
            events.dump()
        );
        let detail = failed.detail.unwrap_or_default();
        assert!(!detail.contains(r"\\localhost"), "{detail}");
        assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::Failed);
        assert_eq!(support_of(&host, matter), Some(FolderSupportKind::Network));
        host.shutdown();
        assert_eq!(envs.lock().unwrap().len(), 0, "the agent was launched");
    }

    /// A matter folder the lawyer can't re-permission: the create puts the answer on the row
    /// before any launch; Codex starts there all the same; and when its shell's write there
    /// fails (a failed tool call), the row still says why.
    #[test]
    fn codex_starts_where_permissions_cant_change_and_the_row_says_why() {
        let dir = Dir::new(
            "folder_support_cant_change",
            &[(CODEX, Scenario::Local("codex-shell-write-denied"))],
        );
        let folder = dir.folder("m", &[("brief.txt", "x\n")]);
        clauseo_sandbox::windows::test_dacl::protect_with(&folder, CANT_CHANGE).unwrap();
        let launcher = Recording::new(&dir);
        let envs = Arc::clone(&launcher.envs);
        let host = dir.start_launching(0, Arc::new(launcher), |config| config);
        let matter = host.create_matter("m", &folder);
        let cant = Some(FolderSupportKind::CantChangePermissions);
        assert_eq!(support_of(&host, matter), cant, "at the create");
        let thread = host.create_thread(matter, CODEX);
        let mut events = host.events(StreamId::Thread(thread));
        let turn = run_turn(&host, &mut events, thread);
        events.assert_no_mismatch();
        assert!(turn_failed(&events, turn).is_none(), "{}", events.dump());
        assert_eq!(
            events.turn_ended(turn).unwrap().stop,
            StopReason::EndTurn,
            "{}",
            events.dump()
        );
        let write_failed = events.count(|event| {
            matches!(event, DomainEvent::ToolCallUpdated(u)
                if u.patch.status == Some(ToolCallStatus::Failed))
        });
        assert_eq!(write_failed, 1, "{}", events.dump());
        assert_eq!(support_of(&host, matter), cant, "after the failed write");
        host.shutdown();
        assert_eq!(envs.lock().unwrap().len(), 1, "Codex was not launched");
    }
}
