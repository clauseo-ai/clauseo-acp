//! What WSL's self-healing at launch tells the host (`wsl_heal`): its progress while it waits and
//! what it did. No Windows call here: the launch check's result carries it on every OS.

/// What the launch check tells the host while it waits, for the host to show; the app owns the
/// words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchProgress {
    /// WSL's pending update is being installed before the launch, which can take a minute or
    /// two: "Updating a Windows component before starting…". Given at most once per check, when
    /// the wait for the upgrade begins; the check returns when it is over.
    UpdatingWindowsComponent,
}

/// What the self-healing did, for the host's log ([`WslHealed::log_line`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WslHealed {
    /// Nothing was owed: the deny was in place, or there is no WSL.
    #[default]
    Nothing,
    /// The deny was missing, the WSL deny task put it back, and the launch went on.
    DenyRestored,
    /// A WSL upgrade was pending; it was installed on the lawyer's behalf (the WSL deny task's
    /// SYSTEM run, which this check requested), the deny is in place, and the launch went on.
    Upgraded,
    /// A WSL upgrade was pending; the WSL deny task tried it on the lawyer's behalf and it failed
    /// ("upgrade failed, deny applied", exit 52): WSL is as it was, the deny verified on it.
    UpgradeFailed,
    /// A WSL upgrade was pending and isn't installed: the task couldn't be started, its run
    /// ended without it, or the wait ran out with no run going. The deny was verified, so the
    /// launch went on.
    UpgradeUnfinished,
}

impl WslHealed {
    /// The line the host logs for it (host.log), or `None` when nothing was done.
    pub fn log_line(self) -> Option<&'static str> {
        match self {
            WslHealed::Nothing => None,
            WslHealed::DenyRestored => {
                Some("WSL's deny for the sandbox users was missing; the WSL deny task put it back")
            }
            WslHealed::Upgraded => Some(
                "WSL's pending update was installed on the lawyer's behalf before the launch, \
                 and WSL's deny for the sandbox users re-applied",
            ),
            WslHealed::UpgradeFailed => Some(
                "WSL's pending update failed when installed on the lawyer's behalf; WSL is as it \
                 was, and WSL's deny for the sandbox users was verified",
            ),
            WslHealed::UpgradeUnfinished => Some(
                "WSL's pending update wasn't installed before the launch; WSL's deny for the \
                 sandbox users was verified",
            ),
        }
    }
}
