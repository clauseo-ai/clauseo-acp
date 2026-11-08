//! The app's side of the set-up task: what it makes of the task's answer, when it falls back
//! to the prompt, and what a pipe someone else serves leads to.

use super::*;

#[test]
fn the_app_maps_the_tasks_answer_and_falls_back_to_the_prompt_without_a_task_or_on_a_refusal() {
    use crate::SetupError;
    let prompted = std::cell::Cell::new(0);
    let elevated = || {
        prompted.set(prompted.get() + 1);
        Err(SetupError::Refused)
    };
    assert_eq!(via_task(Ok(done()), elevated), Ok(()));
    let unsupported = Reply::Unsupported("protocol 2".to_owned());
    assert!(matches!(
        via_task(Ok(unsupported), elevated),
        Err(SetupError::Failed(_))
    ));
    for error in [
        SetUpError::Pipe("broken".to_owned()),
        SetUpError::Task("0x80070005".to_owned()),
        SetUpError::Request("an extend by 40".to_owned()),
    ] {
        let Err(SetupError::Failed(words)) = via_task(Err(error.clone()), elevated) else {
            panic!("{error:?}");
        };
        assert_eq!(words, error.to_string());
    }
    // No server, no published name, a squat: unreachable, for the host's own cause.
    for error in [
        SetUpError::NoServer,
        SetUpError::Unpublished,
        SetUpError::Squatted("the pipe isn't owned by LOCAL SYSTEM".to_owned()),
        SetUpError::NotTheServer("a pool user's".to_owned()),
    ] {
        assert_eq!(
            via_task(Err(error.clone()), elevated),
            Err(SetupError::Unreachable),
            "{error:?}"
        );
    }
    assert_eq!(
        prompted.get(),
        0,
        "no prompt while the task answers or can't be asked"
    );
    // No set-up task (an install from before it): the prompt's path, its refusal kept.
    assert_eq!(
        via_task(Err(SetUpError::NotRegistered), elevated),
        Err(SetupError::Refused)
    );
    assert_eq!(prompted.get(), 1);
    // The task refused this user: the prompt, which needs an administrator, is asked instead.
    let refused = Reply::Refused(Refusal::NotInteractive.to_string());
    assert_eq!(via_task(Ok(refused), elevated), Err(SetupError::Refused));
    assert_eq!(prompted.get(), 2);
    assert_eq!(
        via_task(Ok(Reply::Refused("x".to_owned())), || Ok(())),
        Ok(())
    );
}

#[test]
fn a_squatted_pipe_stops_the_pool_and_asks_once_more_then_refuses_naming_the_squat() {
    use std::cell::{Cell, RefCell};
    let squat = || {
        Err(SetUpError::NotTheServer(
            "the pipe isn't owned by LOCAL SYSTEM".into(),
        ))
    };
    // Each case: what each ask answers, whether the stop starts; then the asks made, the stops
    // asked for, the settles waited, and the answer.
    let run = |answers: Vec<Result<Reply, SetUpError>>, stops: bool| {
        let answers = RefCell::new(answers.into_iter());
        let (asked, stopped, settled) = (Cell::new(0), Cell::new(0), Cell::new(0));
        let answer = retry_squatted(
            || {
                asked.set(asked.get() + 1);
                answers.borrow_mut().next().unwrap()
            },
            || {
                stopped.set(stopped.get() + 1);
                stops
            },
            || settled.set(settled.get() + 1),
        );
        (asked.get(), stopped.get(), settled.get(), answer)
    };
    // The squatter was a pool user's process: the stop frees the name, and the task answers.
    assert_eq!(run(vec![squat(), Ok(done())], true), (2, 1, 1, Ok(done())));
    // Still someone else's after the stop: refused, naming the squat; once only.
    let (asked, stopped, settled, answer) = run(vec![squat(), squat()], true);
    assert_eq!((asked, stopped, settled), (2, 1, 1));
    let Err(SetUpError::Squatted(why)) = &answer else {
        panic!("{answer:?}");
    };
    assert!(
        why.contains("after the sandbox users' processes were stopped"),
        "{why}"
    );
    let words = answer.unwrap_err().to_string();
    assert!(
        words.contains("squatted pipe") && words.contains("nothing was sent"),
        "{words}"
    );
    // A stop that doesn't start leaves nothing to wait for.
    let (asked, stopped, settled, answer) = run(vec![squat()], false);
    assert_eq!((asked, stopped, settled), (1, 1, 0));
    assert!(matches!(answer, Err(SetUpError::Squatted(_))), "{answer:?}");
    // Any other answer is the first one's, with no stop.
    // A squat no pool user made (another user's, an ACCESS_DENIED open, an unread owner, a SYSTEM
    // pipe whose image differs) is already Squatted: answered at once, nothing stopped.
    for first in [
        Ok(done()),
        Err(SetUpError::NoServer),
        Err(SetUpError::NotRegistered),
        Err(SetUpError::Squatted("the pipe refuses this user".into())),
    ] {
        assert_eq!(run(vec![first.clone()], true), (1, 0, 0, first));
    }
    // The app reads the squat as an unreachable task, with no prompt.
    let refused = via_task(Err(SetUpError::Squatted("x".into())), || Ok(()));
    assert_eq!(refused, Err(crate::SetupError::Unreachable));
}

#[test]
fn only_a_pipe_a_pool_user_or_the_pool_group_owns_is_a_pool_squat() {
    let (pool, groups) = (vec![sid(POOL)], vec![sid(GROUP)]);
    assert!(pool_squat(&Ok(sid(POOL)), &pool, &groups));
    assert!(pool_squat(&Ok(sid(GROUP)), &pool, &groups));
    for owner in [
        Ok(sid(OTHER)),
        Ok(sid(LAWYER)),
        Ok(sid("S-1-5-32-544")),
        Ok(sid("S-1-12-1-3263492174-1177263218-2391161522-1513380577")),
        Err("access denied".to_owned()),
    ] {
        assert!(!pool_squat(&owner, &pool, &groups), "{owner:?}");
    }
    // No pool recorded (unread, or none yet): nothing is a pool squat.
    assert!(!pool_squat(&Ok(sid(POOL)), &[], &[]));
}

#[test]
fn the_set_up_asks_for_an_admin_unless_the_task_is_there_enabled_and_serves_the_folder() {
    let serves = Ok(());
    let elsewhere = Err("the app's data folder isn't AppData\\Local\\ai.clauseo.desktop");
    assert!(!asks_admin(Ok(true), serves), "the task, with no prompt");
    // A disabled task, no task (an install from before it, or \Clauseo gone), one that can't be
    // read, or a data folder the task wouldn't set up: the prompt.
    assert!(asks_admin(Ok(false), serves));
    assert!(asks_admin(
        Err("this install has no set-up task".to_owned()),
        serves
    ));
    assert!(asks_admin(
        Err("ITaskFolder::GetTask failed".to_owned()),
        serves
    ));
    assert!(asks_admin(Ok(true), elsewhere));
    assert!(asks_admin(
        Ok(true),
        Err("this user has no profile folder of their own")
    ));
    // The folder half is the task's own rule: the profile's AppData\Local\ai.clauseo.desktop.
    let profile = Path::new(r"C:\Users\lawyer");
    let ours = profile.join(r"AppData\Local\ai.clauseo.desktop");
    assert!(!asks_admin(Ok(true), task_serves(Some(profile), &ours)));
    let moved = Path::new(r"D:\Data\ai.clauseo.desktop");
    assert!(asks_admin(Ok(true), task_serves(Some(profile), moved)));
    assert!(asks_admin(Ok(true), task_serves(None, &ours)));
}
