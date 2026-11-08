//! `repair_install`'s and `repair_cancel`'s admission (0037; both go through `admit_repair`): the
//! main window alone, which shows the Safe workspace's card. The settings window never runs an
//! installer, nor does any other window. While a repair runs, `InstallUpdate` is refused: two
//! installers at once.

use super::*;

/// `repair_install` and its Cancel, `repair_cancel`, from `main` only (and granted to `main` only:
/// `commands::tests::the_settings_window_is_allowed_exactly_its_commands`).
#[test]
fn only_the_main_window_may_run_the_installer() {
    assert_eq!(admit_repair(crate::MAIN_WINDOW), Ok(()));
    for label in [SETTINGS_WINDOW, "agent-browser", "", "Main"] {
        let refused = admit_repair(label).unwrap_err();
        assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{label}");
    }
}

/// While a repair runs, main's "Restart to update" is refused as `command.conflict`; taking a
/// scheduled install back still passes, and once no repair runs the install passes again.
#[test]
fn no_update_installs_while_a_repair_runs() {
    use clauseo_protocol::InstallWhen;
    let scratch = Scratch::new("admit-repair-install", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let install = HostCommand::InstallUpdate {
        when: InstallWhen::Now,
    };
    crate::update::repair::set_running(true);
    let refused = super::super::admit::admit(&bridge, crate::MAIN_WINDOW, &install).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandConflict);
    assert_eq!(
        super::super::admit::admit(
            &bridge,
            crate::MAIN_WINDOW,
            &HostCommand::CancelScheduledUpdate {}
        ),
        Ok(())
    );
    crate::update::repair::set_running(false);
    assert_eq!(
        super::super::admit::admit(&bridge, crate::MAIN_WINDOW, &install),
        Ok(())
    );
}
