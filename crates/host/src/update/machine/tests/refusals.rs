//! Every command in every state: taken where R23's table takes it, refused elsewhere with the
//! reason the actor turns into an error code, and a refusal changes nothing.

use super::*;

/// Each state with its name.
fn states() -> Vec<(&'static str, Machine)> {
    vec![
        ("idle", idle()),
        ("checking", checking()),
        ("downloading", downloading()),
        ("ready", ready().0),
        ("scheduled", scheduled()),
        ("stopping turns", stopping_turns()),
        ("installing", began()),
        ("failed", failed()),
        ("managed", managed()),
    ]
}

/// The states that take `input`: every other state refuses it with `NotNow`.
fn takers(input: &Input) -> &'static [&'static str] {
    const ALL: &[&str] = &[
        "idle",
        "checking",
        "downloading",
        "ready",
        "scheduled",
        "stopping turns",
        "installing",
        "failed",
        "managed",
    ];
    match input {
        Input::CheckRequested => &["idle", "failed"],
        Input::InstallRequested(_) => &["ready"],
        Input::CancelScheduled => &["scheduled", "stopping turns"],
        Input::ModeSet(UpdateMode::None) => &[],
        Input::ModeSet(_) => ALL,
        Input::TimerFired
        | Input::CheckDone(_)
        | Input::Progress(_)
        | Input::DownloadDone(_)
        | Input::PolicyChanged(_)
        | Input::TurnsRunning(_)
        | Input::TurnsCancelled => panic!("not a command: {input:?}"),
    }
}

fn commands() -> Vec<Input> {
    vec![
        Input::CheckRequested,
        Input::InstallRequested(InstallWhen::Now),
        Input::InstallRequested(InstallWhen::AfterTurn),
        Input::CancelScheduled,
        Input::ModeSet(UpdateMode::Auto),
        Input::ModeSet(UpdateMode::Notify),
        Input::ModeSet(UpdateMode::None),
    ]
}

#[test]
fn update_every_command_is_refused_in_each_state_that_does_not_take_it() {
    let mut refused = 0;
    for (name, m) in states() {
        for input in commands() {
            let takes = takers(&input).contains(&name);
            let case = format!("{name} + {input:?}");
            match step(&m, input, t0()) {
                Ok(_) => assert!(takes, "{case} was taken"),
                Err(why) => {
                    assert!(!takes, "{case} was refused");
                    assert_eq!(why, Refusal::NotNow, "{case}");
                    refused += 1;
                }
            }
        }
    }
    // 9 states x 7 commands, less the 24 pairs taken.
    assert_eq!(refused, 9 * 7 - 24);
}

#[test]
fn update_a_refusal_leaves_the_machine_as_it_was() {
    let m = scheduled();
    assert!(step(&m, Input::CheckRequested, t0()).is_err());
    // `step` borrows: the machine the caller holds is the one to keep.
    let (next, _) = go(&m, Input::CancelScheduled, t0());
    assert!(matches!(next.view().state, UpdateState::Ready { .. }));
}

#[test]
fn update_mode_set_while_policy_locks_the_mode_is_locked_in_every_state() {
    // The actor answers `settings.locked` (R23 PR 4), as for any other locked key.
    let locked = |chosen| ModeInForce {
        mode: chosen,
        locked: true,
    };
    for (name, m) in states() {
        for policy in [UpdateMode::Auto, UpdateMode::None] {
            let (m, _) = go(&m, Input::PolicyChanged(locked(policy)), t0());
            for chosen in UpdateMode::ALL {
                let why = step(&m, Input::ModeSet(chosen), t0()).err();
                assert_eq!(why, Some(Refusal::Locked), "{name}, {policy:?}, {chosen:?}");
            }
        }
    }
}

#[test]
fn update_managed_by_policy_refuses_check_and_install_and_mode_set() {
    let start = Start {
        mode: ModeInForce {
            mode: UpdateMode::None,
            locked: true,
        },
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::None))
    };
    let m = Machine::start(start, t0()).0;
    let policy = UpdateState::Managed {
        by: ManagedBy::Policy,
    };
    assert_eq!(m.view().state, policy);
    for input in commands() {
        let expected = if matches!(input, Input::ModeSet(_)) {
            Refusal::Locked
        } else {
            Refusal::NotNow
        };
        assert_eq!(step(&m, input, t0()).err(), Some(expected));
    }
}
