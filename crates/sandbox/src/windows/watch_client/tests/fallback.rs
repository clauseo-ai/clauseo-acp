//! The fallback states, the second request, the restart rate and the server check.

use super::*;
use crate::windows::watch_client::verify::{ServerFacts, check_server};

/// Registers once with the fake scripted by `set`, lets the supervisor try for `minutes` of fake
/// time, and gives the status it settled on.
fn down_with(set: impl FnOnce(&mut Script), minutes: u64) -> (Fake, Status) {
    let fake = Fake::default();
    set(&mut fake.script());
    let hub = Hub::new(fake.clone());
    let dir = TempDir::new("watch-client-down");
    let a = launch();
    let registration = hub.register(sid(POOL_A), policy(&dir), Arc::clone(&a.sink));
    let status = hub.status();
    eventually("the supervisor kept trying", || {
        fake.now() >= Duration::from_secs(60 * minutes)
    });
    drop(registration);
    assert_eq!(hub.status(), Status::Off);
    (fake, status)
}

#[test]
fn a_task_not_registered_is_down_and_logged_once_however_often_it_is_retried() {
    let (fake, status) = down_with(|s| s.request = Err(StartError::NotRegistered), 3);
    assert_eq!(status, Status::Down(Down::NotRegistered));
    assert!(fake.requests().len() >= 3);
    assert_eq!(fake.errors().len(), 1, "{:?}", fake.errors());
}

#[test]
fn a_watcher_that_exits_68_is_provider_full_after_one_second_request() {
    let (fake, status) = down_with(
        |s| {
            s.name = Ok(None);
            s.last_exit = Some(PROVIDER_FULL);
        },
        0,
    );
    assert_eq!(status, Status::Down(Down::ProviderFull));
    let requests = fake.requests();
    let (first, second) = (requests[0], requests[1]);
    let gap = second.saturating_sub(first);
    assert!(gap >= RETRIGGER_AFTER && gap < PIPE_WAIT);
    assert!(fake.errors()[0].contains("68"));
}

#[test]
fn no_name_is_no_pipe_and_another_exit_is_named() {
    let (_, status) = down_with(|s| s.name = Ok(None), 0);
    assert_eq!(status, Status::Down(Down::NoPipe));
    let (fake, status) = down_with(
        |s| {
            s.name = Ok(None);
            s.last_exit = Some(74);
        },
        0,
    );
    assert_eq!(status, Status::Down(Down::NoPipe));
    assert!(fake.errors()[0].contains("exited 74"));
    let requests = fake.requests();
    let gap = requests[1].saturating_sub(requests[0]);
    assert!(
        gap >= RETRIGGER_AFTER && gap < PIPE_WAIT,
        "one second request, then the fallback"
    );
    // 71: the machine has no record (set-up never ran, or was removed).
    let (fake, status) = down_with(
        |s| {
            s.name = Ok(None);
            s.last_exit = Some(71);
        },
        0,
    );
    assert_eq!(status, Status::Down(Down::NoPipe));
    assert!(fake.errors()[0].contains("exited 71"));
}

#[test]
fn a_server_that_fails_the_check_or_a_key_not_systems_is_refused() {
    let (fake, status) = down_with(
        |s| {
            for _ in 0..1000 {
                let refused = Err(ConnectError::NotTheServer("not SYSTEM".to_owned()));
                s.connects.push_back(refused);
            }
        },
        0,
    );
    assert_eq!(status, Status::Down(Down::NotTheServer));
    // The refused name is polled past, not opened again: one open per attempt, and each
    // attempt makes its request first.
    let opened = fake.0.connects.lock().unwrap().len();
    assert!(opened <= fake.requests().len(), "{opened}");

    let (fake, status) = down_with(|s| s.name = Err("the key isn't SYSTEM's".to_owned()), 0);
    assert_eq!(status, Status::Down(Down::NotTheServer));
    assert!(
        fake.0.connects.lock().unwrap().is_empty(),
        "a name not trusted isn't opened"
    );
}

#[test]
fn a_name_that_comes_late_is_found_after_the_second_request() {
    let fake = Fake::default();
    fake.script().name_from = Duration::from_millis(2500);
    let _pipe = fake.pipe();
    let hub = Hub::new(fake.clone());
    let dir = TempDir::new("watch-client-late");
    let a = launch();
    let _a = hub.register(sid(POOL_A), policy(&dir), Arc::clone(&a.sink));
    assert_eq!(hub.status(), Status::Live);
    assert_eq!(fake.requests().len(), 2);
}

#[test]
fn a_closed_pipe_is_logged_each_time_and_restarted_at_most_once_a_minute() {
    let fake = Fake::default();
    for _ in 0..4 {
        // Each pipe ends at once: its sender is dropped.
        drop(fake.pipe());
    }
    let hub = Hub::new(fake.clone());
    let dir = TempDir::new("watch-client-restart");
    let a = launch();
    let registration = hub.register(sid(POOL_A), policy(&dir), Arc::clone(&a.sink));
    eventually("three restarts", || fake.requests().len() >= 4);
    drop(registration);
    let requests = fake.requests();
    for pair in requests.windows(2).take(3) {
        assert!(
            pair[1].saturating_sub(pair[0]) >= RESTART_EVERY,
            "{requests:?}"
        );
    }
    let closed = fake.errors();
    // The first three ends; a later attempt, past the scripted pipes, finds none.
    assert!(closed.len() >= 3, "{closed:?}");
    assert!(
        closed.iter().take(3).all(|line| line.contains("closed")),
        "{closed:?}"
    );
}

fn facts() -> ServerFacts {
    ServerFacts {
        owner: Ok(sid("S-1-5-18")),
        session: Ok(0),
        user: Ok(sid("S-1-5-18")),
        image: Ok(r"\Device\HarddiskVolume3\Program Files\Clauseo\Sandbox\setup.exe".to_owned()),
    }
}

#[test]
fn the_server_check_needs_systems_owner_and_takes_unread_facts_on_its_word() {
    let exe = Path::new(r"C:\Program Files\Clauseo\Sandbox\setup.exe");
    let device = r"\Device\HarddiskVolume3";
    assert_eq!(check_server(&facts(), exe, device), Ok(Vec::new()));

    let squatter = ServerFacts {
        owner: Ok(sid(POOL_A)),
        ..facts()
    };
    assert!(check_server(&squatter, exe, device).is_err());
    let unread_owner = ServerFacts {
        owner: Err("denied".to_owned()),
        ..facts()
    };
    assert!(check_server(&unread_owner, exe, device).is_err());

    // The process's user and image unread: accepted on the owner, and said.
    let unread = ServerFacts {
        user: Err("access denied".to_owned()),
        image: Err("the kernel won't give it".to_owned()),
        session: Err("denied".to_owned()),
        ..facts()
    };
    assert_eq!(check_server(&unread, exe, device).unwrap().len(), 3);

    for wrong in [
        ServerFacts {
            user: Ok(sid(POOL_A)),
            ..facts()
        },
        ServerFacts {
            session: Ok(1),
            ..facts()
        },
        ServerFacts {
            image: Ok(r"\Device\HarddiskVolume3\Users\x\setup.exe".to_owned()),
            ..facts()
        },
    ] {
        assert!(check_server(&wrong, exe, device).is_err(), "{wrong:?}");
    }
}

#[test]
fn turned_off_by_policy_never_starts_the_task_and_is_logged_once() {
    let (fake, status) = down_with(|s| s.off = true, 3);
    assert_eq!(status, Status::Down(Down::TurnedOff));
    assert_eq!(fake.requests(), []);
    let errors = fake.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("turned off by policy"));
}

#[test]
fn the_off_switch_fails_closed_on_a_value_of_another_type_or_one_unreadable() {
    use crate::windows::watch_client::policy_says_off;
    // On: absent, or a DWORD other than 0.
    assert!(!policy_says_off::<()>(&Ok(None)));
    assert!(!policy_says_off::<()>(&Ok(Some(1))));
    assert!(!policy_says_off::<()>(&Ok(Some(u32::MAX))));
    // Off: a DWORD 0, a value of another type (a REG_SZ "0"), or one that can't be read.
    assert!(policy_says_off::<()>(&Ok(Some(0))));
    assert!(policy_says_off(&Err("wrong type")));
    assert!(policy_says_off(&Err("access denied")));
}
