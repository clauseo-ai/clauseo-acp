//! The download's guardrails through the host: the firm's setting forbids it, and the daily
//! check at start updates a copy more than one release behind (saying so) and removes the old
//! copy.

use clauseo_host::browser::install::fresh::{self, StableRecord};
use clauseo_protocol::{BrowserInstallFailure, BrowserInstallState, CommandOutcome, HostCommand};

use super::{Fake, MAC_BINARY, browsers, start, states, subscribe};
use crate::support::Dir;
use crate::write;

/// With the firm's `browser.allow_download` false, `InstallBrowser` answers `Done`, the app hears
/// `Failed { firm_forbids }`, and nothing downloads.
#[test]
fn the_firms_setting_forbids_the_download() {
    let dir = Dir::new("browser_install_forbidden", &[]);
    dir.set_policy(r#"{"browser": {"allow_download": false}}"#);
    let (tools, _handles) = Fake::new(false);
    let host = start(&dir, std::sync::Arc::clone(&tools));
    let mut sub = subscribe(&host);
    assert_eq!(
        host.ok(HostCommand::InstallBrowser {}),
        CommandOutcome::Done
    );
    let heard = states(&mut sub);
    assert!(
        matches!(
            heard.as_slice(),
            [BrowserInstallState::Failed {
                code: BrowserInstallFailure::FirmForbids,
                ..
            }]
        ),
        "{heard:?}"
    );
    assert_eq!(tools.downloads.load(std::sync::atomic::Ordering::SeqCst), 0);
    host.shutdown();
}

/// A copy two Stable releases behind: the check at start downloads the new one with `behind`
/// set, then the old copy goes, since no session runs it.
#[test]
fn the_check_at_start_updates_a_copy_two_releases_behind() {
    let dir = Dir::new("browser_install_behind", &[]);
    let browsers_dir = dir.data().join("app").join("browsers");
    let old = MAC_BINARY
        .split('/')
        .fold(browsers_dir.join("chrome-158.0.0.1"), |path, part| {
            path.join(part)
        });
    write::create_dir_all(old.parent().unwrap());
    write::write(&old, b"chrome");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let seen = StableRecord {
        stable: "158.0.0.5".to_owned(),
        previous: Some("158.0.0.4".to_owned()),
        checked_at: i64::try_from(now).unwrap(),
    };
    fresh::write_record(&browsers_dir, &seen).unwrap();
    let (tools, handles) = Fake::new(true);
    let host = start(&dir, tools);
    handles
        .starts
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the check at start downloads");
    let mut sub = subscribe(&host);
    handles.release.send(()).unwrap();
    let heard = states(&mut sub);
    assert!(
        heard
            .iter()
            .any(|state| matches!(state, BrowserInstallState::Downloading { behind: true, .. })),
        "{heard:?}"
    );
    assert_eq!(heard.last(), Some(&BrowserInstallState::Done));
    host.shutdown();
    assert_eq!(browsers(&dir), ["chrome-158.0.6778.85"]);
}
