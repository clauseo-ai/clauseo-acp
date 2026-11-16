//! The hook in temp homes with fake, non-credential `auth.json` files at mode 0000, so any open
//! would fail: the link holds (`Shared`, and `Yes`), a rename-save over the home's file splits
//! it, the lawyer's file gone is `Gone`, a symbolic link is `Split` to the check and `Yes` to
//! `shared`, and a login not reused is `Own` only when the files are two. With no home for the
//! lawyer, or a stat that fails, every answer fails closed.

use std::path::PathBuf;

use clauseo_host::{LoginCheck, LoginLink, Shared};
use clauseo_protocol::AgentId;

use super::{DesktopLoginCheck, ReusedLogins};
use crate::test_support::Scratch;

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

/// A scratch with the agent's home and the lawyer's `.codex` folder made.
struct Homes {
    scratch: Scratch,
}

impl Homes {
    fn new(test: &str) -> Self {
        Self {
            scratch: Scratch::folder(test, &["agent-home", "lawyer/.codex"]),
        }
    }
    fn agent(&self) -> PathBuf {
        self.scratch.path().join("agent-home")
    }
}

/// The paths and the check the unix tests use (the fake files need unix modes).
#[cfg(unix)]
trait UnixHomes {
    fn lawyer(&self) -> PathBuf;
    fn lawyers_file(&self) -> PathBuf;
    fn home_file(&self) -> PathBuf;
    fn check(&self, reused: bool) -> DesktopLoginCheck;
}

#[cfg(unix)]
impl UnixHomes for Homes {
    fn lawyer(&self) -> PathBuf {
        self.scratch.path().join("lawyer")
    }
    fn lawyers_file(&self) -> PathBuf {
        self.lawyer().join(".codex").join("auth.json")
    }
    fn home_file(&self) -> PathBuf {
        self.agent().join("auth.json")
    }
    fn check(&self, reused: bool) -> DesktopLoginCheck {
        let logins = ReusedLogins::default();
        logins.set(&codex(), reused);
        DesktopLoginCheck::new(Some(self.lawyer()), logins)
    }
}

#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "test set-up writes fake login files; the desktop crate has no write module"
)]
mod unix {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    use super::*;

    /// A fake login file at `path`, mode 0000.
    fn fake_login(path: &Path) {
        std::fs::write(path, b"not a credential").unwrap();
        locked(path);
    }

    fn locked(path: &Path) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    }

    #[test]
    fn the_link_holds_then_splits_on_a_rename_save_then_the_lawyer_s_file_goes() {
        let homes = Homes::new("login-check-split");
        fake_login(&homes.lawyers_file());
        std::fs::hard_link(homes.lawyers_file(), homes.home_file()).unwrap();
        let check = homes.check(true);
        assert_eq!(check.check(&codex(), &homes.agent()), LoginLink::Shared);
        assert_eq!(check.shared(&codex(), &homes.agent()), Shared::Yes);
        // A rename-save over the home's file: written beside, renamed over.
        let beside = homes.agent().join("auth.json.tmp");
        fake_login(&beside);
        std::fs::rename(&beside, homes.home_file()).unwrap();
        assert_eq!(check.check(&codex(), &homes.agent()), LoginLink::Split);
        assert_eq!(check.shared(&codex(), &homes.agent()), Shared::No);
        std::fs::remove_file(homes.lawyers_file()).unwrap();
        assert_eq!(check.check(&codex(), &homes.agent()), LoginLink::Gone);
    }

    #[test]
    fn a_symbolic_link_is_split_to_the_check_and_shared_to_the_guard() {
        let homes = Homes::new("login-check-symlink");
        fake_login(&homes.lawyers_file());
        std::os::unix::fs::symlink(homes.lawyers_file(), homes.home_file()).unwrap();
        assert_eq!(
            homes.check(true).check(&codex(), &homes.agent()),
            LoginLink::Split
        );
        assert_eq!(
            homes.check(false).check(&codex(), &homes.agent()),
            LoginLink::Shared,
            "not reused: a link is never Own"
        );
        assert_eq!(
            homes.check(false).shared(&codex(), &homes.agent()),
            Shared::Yes
        );
    }

    #[test]
    fn a_login_not_reused_is_own_only_when_the_files_are_two() {
        let homes = Homes::new("login-check-own");
        let check = homes.check(false);
        assert_eq!(
            check.check(&codex(), &homes.agent()),
            LoginLink::Own,
            "none"
        );
        fake_login(&homes.lawyers_file());
        fake_login(&homes.home_file());
        assert_eq!(check.check(&codex(), &homes.agent()), LoginLink::Own, "two");
        assert_eq!(check.shared(&codex(), &homes.agent()), Shared::No);
        std::fs::remove_file(homes.home_file()).unwrap();
        std::fs::hard_link(homes.lawyers_file(), homes.home_file()).unwrap();
        assert_eq!(
            check.check(&codex(), &homes.agent()),
            LoginLink::Shared,
            "one"
        );
    }

    #[test]
    fn a_stat_that_fails_is_unknown_to_the_guard_and_never_own() {
        // A search-denied folder fails every stat inside it (root reads it all the same).
        if is_root() {
            return;
        }
        let homes = Homes::new("login-check-stat");
        fake_login(&homes.lawyers_file());
        fake_login(&homes.home_file());
        let folder = homes.agent();
        locked(&folder);
        let check = homes.check(false);
        let shared = check.shared(&codex(), &folder);
        let link = check.check(&codex(), &folder);
        let reused = homes.check(true).check(&codex(), &folder);
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(shared, Shared::Unknown);
        assert_eq!(link, LoginLink::Shared);
        assert_eq!(reused, LoginLink::Split);
    }

    /// Whether this test runs as root, for whom a mode never denies a stat.
    fn is_root() -> bool {
        std::fs::metadata("/proc/self")
            .is_ok_and(|meta| std::os::unix::fs::MetadataExt::uid(&meta) == 0)
    }
}

#[test]
fn with_no_home_for_the_lawyer_every_answer_fails_closed() {
    let homes = Homes::new("login-check-no-home");
    for reused in [false, true] {
        let logins = ReusedLogins::default();
        logins.set(&codex(), reused);
        let check = DesktopLoginCheck::new(None, logins);
        let link = check.check(&codex(), &homes.agent());
        assert_eq!(
            link,
            if reused {
                LoginLink::Split
            } else {
                LoginLink::Shared
            }
        );
        assert_eq!(check.shared(&codex(), &homes.agent()), Shared::Unknown);
    }
}
