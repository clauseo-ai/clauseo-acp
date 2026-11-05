//! The download's state in every app catch-up: a subscriber that lagged, or one that subscribes
//! mid-download or after the end, hears where the download stands (the host review of #337).

use std::time::Duration;

use clauseo_host::{AppSubscription, HostConfig};
use clauseo_protocol::{BrowserInstallState, Count, HostCommand, HostEvent};
use futures::StreamExt as _;
use futures::future::{self, Either};

use super::{Fake, browsers, roots, subscribe};
use crate::support::{Dir, Running};

/// How long a read waits for one more event before taking the stream as quiet.
const QUIET: Duration = Duration::from_millis(300);

/// Fresh subscriptions tried before giving up on seeing a state in the first frames.
const TRIES: usize = 50;

/// Every event `sub` holds now, until it is quiet for [`QUIET`].
pub(super) fn available(sub: &mut AppSubscription) -> Vec<HostEvent> {
    let mut events = Vec::new();
    loop {
        let next = futures::executor::block_on(async {
            let next = std::pin::pin!(sub.next());
            match future::select(next, async_io::Timer::after(QUIET)).await {
                Either::Left((item, _)) => item,
                Either::Right(_) => None,
            }
        });
        let Some(item) = next else {
            return events;
        };
        events.push(crate::settings::decode(&item));
    }
}

/// The download states among `events`, in order.
pub(super) fn install_states(events: &[HostEvent]) -> Vec<BrowserInstallState> {
    events
        .iter()
        .filter_map(|event| match event {
            HostEvent::BrowserInstall { state } => Some(state.clone()),
            _ => None,
        })
        .collect()
}

/// Subscribes afresh until the first frames' download state is `state` (the catch-up comes
/// before anything live), which also shows the actor has taken it in. Panics after [`TRIES`].
fn first_frames_carry(host: &Running, state: &BrowserInstallState) -> Vec<HostEvent> {
    for _ in 0..TRIES {
        let mut sub = subscribe(host);
        let frames = available(&mut sub);
        if install_states(&frames).first() == Some(state) {
            return frames;
        }
    }
    panic!("no fresh subscription's first frames carried {state:?}");
}

fn downloading() -> BrowserInstallState {
    BrowserInstallState::Downloading {
        bytes: Count::from(22u32),
        total: Count::from(22u32),
    }
}

/// The host review's repro: with room for one event, a subscriber left unread lags while the
/// download ends; once it reads, its catch-up carries the end (`Done`), which before the fix
/// it never heard. Then a second download starts and its cancel reaches it too.
#[test]
fn a_lagged_subscriber_hears_how_the_download_ended() {
    let dir = Dir::new("browser_install_lagged", &[]);
    let (tools, handles) = Fake::new(true);
    let found = roots(&dir);
    let host = dir.start_with(0, move |config: HostConfig| {
        config
            .with_browser_install_tools(tools)
            .with_chrome_roots(found)
            .with_app_subscription_capacity(1)
    });
    let mut lagged = subscribe(&host); // not read until the download has ended
    host.ok(HostCommand::InstallBrowser {});
    handles
        .starts
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    handles.release.send(()).unwrap();
    first_frames_carry(&host, &BrowserInstallState::Done);
    assert_eq!(browsers(&dir), ["chrome-131.0.6778.85"]);
    let seen = install_states(&available(&mut lagged));
    assert_eq!(
        seen.last(),
        Some(&BrowserInstallState::Done),
        "the lagged subscriber's states: {seen:?}"
    );
    // A second download: the subscriber, caught up, hears it start and end.
    host.ok(HostCommand::InstallBrowser {});
    handles
        .starts
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    host.ok(HostCommand::CancelBrowserInstall {});
    first_frames_carry(&host, &cancelled());
    let seen = install_states(&available(&mut lagged));
    assert_eq!(seen.last(), Some(&cancelled()), "{seen:?}");
    host.shutdown();
}

fn cancelled() -> BrowserInstallState {
    BrowserInstallState::Failed {
        code: clauseo_protocol::BrowserInstallFailure::Cancelled,
        message: "the download was cancelled".to_owned(),
    }
}

/// A window that subscribes mid-download gets the current step at once, in its first frames;
/// one that subscribes after the end gets `Done`. Before any download the first frames carry no
/// download state.
#[test]
fn a_new_subscriber_hears_where_the_download_stands() {
    let dir = Dir::new("browser_install_fresh", &[]);
    let (tools, handles) = Fake::new(true);
    let found = roots(&dir);
    let host = dir.start_with(0, move |config: HostConfig| {
        config
            .with_browser_install_tools(tools)
            .with_chrome_roots(found)
    });
    let mut before = subscribe(&host);
    assert_eq!(install_states(&available(&mut before)), []);
    host.ok(HostCommand::InstallBrowser {});
    handles
        .starts
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    let frames = first_frames_carry(&host, &downloading());
    assert_eq!(
        install_states(&frames),
        [downloading()],
        "one state, the current one"
    );
    handles.release.send(()).unwrap();
    let frames = first_frames_carry(&host, &BrowserInstallState::Done);
    assert_eq!(install_states(&frames), [BrowserInstallState::Done]);
    host.shutdown();
}
