//! The download's guardrails through the host: the firm's setting forbids it, and the daily
//! check at start updates a copy more than one release behind (saying so) and removes the old
//! copy.

use clauseo_host::browser::install::fresh::{self, StableRecord};
use clauseo_protocol::{BrowserInstallFailure, BrowserInstallState, CommandOutcome, HostCommand};

use super::{Fake, MAC_BINARY, browsers, start, states, subscribe};
use crate::support::Dir;
use crate::write;

fn forbids(state: &BrowserInstallState) -> bool {
    matches!(
        state,
        BrowserInstallState::Failed {
            code: BrowserInstallFailure::FirmForbids,
            ..
        }
    )
}

/// With the firm's `browser.allow_download` false, the app hears `Failed { firm_forbids }` in its
/// first catch-up, before anyone asks, so it never offers the download; `InstallBrowser` answers
/// `Done` and the app hears the refusal again. Nothing is fetched: neither the list nor a zip.
#[test]
fn the_firms_setting_forbids_the_download() {
    let dir = Dir::new("browser_install_forbidden", &[]);
    dir.set_policy(r#"{"browser": {"allow_download": false}}"#);
    let (tools, _handles) = Fake::new(false);
    let host = start(&dir, std::sync::Arc::clone(&tools));
    let mut sub = subscribe(&host);
    let first = states(&mut sub);
    assert!(
        matches!(first.as_slice(), [state] if forbids(state)),
        "the catch-up, before any ask: {first:?}"
    );
    assert_eq!(
        host.ok(HostCommand::InstallBrowser {}),
        CommandOutcome::Done
    );
    let heard = states(&mut sub);
    assert!(
        matches!(heard.as_slice(), [state] if forbids(state)),
        "{heard:?}"
    );
    assert_eq!(tools.downloads.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(tools.fetches.load(std::sync::atomic::Ordering::SeqCst), 0);
    host.shutdown();
}

/// `network.upstream_proxy`, the firm's policy value or the lawyer's own, reaches both the
/// version list's fetch and the zip's download.
#[test]
fn the_upstream_proxy_reaches_the_download() {
    let dir = Dir::new("browser_install_firm_proxy", &[]);
    dir.set_policy(r#"{"network": {"upstream_proxy": "http://proxy.firm.example:3128"}}"#);
    let (tools, _handles) = Fake::new(false);
    let host = start(&dir, std::sync::Arc::clone(&tools));
    let mut sub = subscribe(&host);
    host.ok(HostCommand::InstallBrowser {});
    assert_eq!(states(&mut sub).last(), Some(&BrowserInstallState::Done));
    host.shutdown();
    let firm = Some("http://proxy.firm.example:3128".to_owned());
    assert_eq!(*tools.proxies.lock().unwrap(), [firm.clone(), firm]);

    let dir = Dir::new("browser_install_lawyer_proxy", &[]);
    dir.set_settings(
        &[],
        serde_json::json!({"network": {"upstream_proxy": "http://proxy.lawyer.example:8080"}}),
    );
    let (tools, _handles) = Fake::new(false);
    let host = start(&dir, std::sync::Arc::clone(&tools));
    let mut sub = subscribe(&host);
    host.ok(HostCommand::InstallBrowser {});
    assert_eq!(states(&mut sub).last(), Some(&BrowserInstallState::Done));
    host.shutdown();
    let lawyer = Some("http://proxy.lawyer.example:8080".to_owned());
    assert_eq!(*tools.proxies.lock().unwrap(), [lawyer.clone(), lawyer]);
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
    let seen = StableRecord {
        stable: "158.0.0.5".to_owned(),
        previous: Some("158.0.0.4".to_owned()),
        // Whatever the test clock says, the copy is behind: older than Stable and the one
        // before it, or than Stable alone once that record counts as stale.
        checked_at: 0,
    };
    fresh::write_record(&browsers_dir, &seen).unwrap();
    let (tools, handles) = Fake::new(true);
    let host = start(&dir, tools);
    if handles
        .starts
        .recv_timeout(std::time::Duration::from_secs(20))
        .is_err()
    {
        let record = fresh::read_record(&browsers_dir);
        let listed: Vec<String> = std::fs::read_dir(&browsers_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        panic!("the check at start did not download: record {record:?}, browsers {listed:?}");
    }
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
