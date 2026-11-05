//! The updater's wire before it is wired (decision record 0037, R23 PR 2): each update command
//! is refused with `command.invalid` and changes nothing, and `GetUpdate` answers the view
//! `NoUpdater` will give, managed by the package.

use clauseo_protocol::{
    ErrorCode, HostCommand, HostQuery, InstallWhen, ManagedBy, QueryResult, UpdateMode, UpdateState,
};

use crate::settings::{ready, subscribe_app};
use crate::support::Dir;

#[test]
fn each_update_command_is_refused_until_the_updater_is_wired() {
    let dir = Dir::new("update_not_wired", &[]);
    let host = dir.start(0);
    let (mut sub, _first) = subscribe_app(&host);
    let before = dir.settings_json();
    for command in [
        HostCommand::CheckForUpdate {},
        HostCommand::InstallUpdate {
            when: InstallWhen::Now,
        },
        HostCommand::InstallUpdate {
            when: InstallWhen::AfterTurn,
        },
        HostCommand::CancelScheduledUpdate {},
        HostCommand::SetUpdateMode {
            mode: UpdateMode::Notify,
        },
        HostCommand::SetUpdateMode {
            mode: UpdateMode::Auto,
        },
    ] {
        let error = host.refused(command, ErrorCode::CommandInvalid);
        assert_eq!(error.message(), "the updater is not wired yet");
    }
    assert_eq!(dir.settings_json(), before);
    host.settle();
    assert_eq!(ready(&mut sub), []);
    host.shutdown();
}

#[test]
fn get_update_answers_managed_by_the_package_in_auto() {
    let dir = Dir::new("update_get", &[]);
    let host = dir.start(0);
    let QueryResult::Update(view) = host.query(HostQuery::GetUpdate).unwrap() else {
        panic!("GetUpdate answered something else");
    };
    assert_eq!(
        view.state,
        UpdateState::Managed {
            by: ManagedBy::Package
        }
    );
    assert_eq!(view.mode, UpdateMode::Auto);
    // The host crate's version stands in until PR 4 brings the app's.
    assert_eq!(view.current, env!("CARGO_PKG_VERSION"));
    assert_eq!(view.checked_at, None);
    assert_eq!(view.updated_from, None);
    host.shutdown();
}
