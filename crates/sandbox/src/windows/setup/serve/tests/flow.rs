//! The order each client is served in and what its reply says: a current set-up, an extend,
//! the queue under the lock, the gate again under it, the pool refused before anything is read,
//! the identified user's profile alone, and the server's own exits.

use std::path::{Path, PathBuf};

use super::*;

#[test]
fn a_user_whose_set_up_is_current_gets_done_with_nothing_run() {
    let mut host = FakeHost::new();
    host.current = vec![sid(LAWYER)];
    // A set-up would fail: it isn't reached, and no pool password is reset.
    host.result = SetupResult::failed(SetupExit::Failed, "x", "would have reset passwords");
    let (_, replies, _) = serve_all(&mut host, vec![FakeClient::asking(LAWYER, SET_UP)]);
    assert_eq!(replies, [done()]);
    assert!(
        !host.calls.iter().any(|c| c.starts_with("set_up")),
        "{:?}",
        host.calls
    );
    assert!(!host.held);
}

#[test]
fn an_extend_on_the_pipe_runs_the_extend_path_even_when_current() {
    let mut host = FakeHost::new();
    host.current = vec![sid(LAWYER)];
    let (_, replies, _) = serve_all(
        &mut host,
        vec![FakeClient::asking(LAWYER, r#"{"v":1,"op":"add","n":3}"#)],
    );
    assert_eq!(replies, [done()]);
    let set_up: Vec<&String> = host
        .calls
        .iter()
        .filter(|c| c.starts_with("set_up"))
        .collect();
    assert_eq!(set_up.len(), 1, "{:?}", host.calls);
    assert!(
        set_up[0].starts_with(&format!("set_up {LAWYER} ")),
        "{}",
        set_up[0]
    );
    assert!(set_up[0].ends_with(" 3"), "{}", set_up[0]);
    assert!(
        !host.calls.iter().any(|c| c.starts_with("current")),
        "an extend always runs"
    );
    // Out of range, or not the protocol: refused before anything is read.
    for bad in [
        r#"{"v":1,"op":"add","n":0}"#,
        r#"{"v":1,"op":"add","n":33}"#,
        r#"{"v":1,"op":"add"}"#,
        r#"{"v":1}"#,
        r#"{"op":"set_up"}"#,
        r#"{"v":1,"op":"set_up","add":3}"#,
        r#"["set_up"]"#,
        "{",
    ] {
        let mut host = FakeHost::new();
        let (_, replies, _) = serve_all(&mut host, vec![FakeClient::asking(LAWYER, bad)]);
        assert!(
            matches!(&replies[..], [Reply::Refused(why)] if why.starts_with("the request")),
            "{bad}: {replies:?}"
        );
        assert!(host.acted().is_empty(), "{bad}: {:?}", host.calls);
    }
}

#[test]
fn clients_are_served_one_at_a_time_each_under_the_lock_it_waited_for() {
    let mut host = FakeHost::new();
    let clients = vec![
        FakeClient::asking(LAWYER, SET_UP),
        FakeClient {
            facts: Ok(ClientFacts {
                session: 2,
                ..client(OTHER)
            }),
            ..FakeClient::asking(OTHER, SET_UP)
        },
    ];
    let (exit, replies, waits) = serve_all(&mut host, clients);
    assert_eq!(exit, ServeExit::Done);
    assert_eq!(replies, [done(), done()]);
    assert_eq!(waits, [FIRST_WAIT, IDLE_WAIT, IDLE_WAIT]);
    let order: Vec<&str> = host
        .calls
        .iter()
        .map(String::as_str)
        .filter(|c| ["lock", "unlock"].contains(c) || c.starts_with("set_up"))
        .collect();
    assert_eq!(order.len(), 6, "{order:?}");
    assert_eq!(order[0], "lock");
    assert!(
        order[1].starts_with(&format!("set_up {LAWYER}")),
        "{order:?}"
    );
    assert_eq!((order[2], order[3]), ("unlock", "lock"));
    assert!(
        order[4].starts_with(&format!("set_up {OTHER}")),
        "{order:?}"
    );
    assert_eq!(order[5], "unlock");
    // A lock that stays held past the wait is exit 5, with nothing run.
    let mut host = FakeHost::new();
    host.lock_free = false;
    let (_, replies, _) = serve_all(&mut host, vec![FakeClient::asking(LAWYER, SET_UP)]);
    let [Reply::Ran(result)] = &replies[..] else {
        panic!("{replies:?}")
    };
    assert_eq!(result.exit, SetupExit::LockHeld.as_code());
    assert_eq!(result.step.as_deref(), Some("lock"));
    assert!(!host.calls.iter().any(|c| c.starts_with("set_up")));
}

#[test]
fn the_gate_is_checked_again_under_the_lock() {
    // A removal took the admin copy away while this client waited for the lock.
    let mut host = FakeHost::new();
    host.gate_after_lock = Some(Gate {
        copy_owner: None,
        ..machine_gate()
    });
    let (_, replies, _) = serve_all(&mut host, vec![FakeClient::asking(LAWYER, SET_UP)]);
    assert_eq!(replies, [Reply::Refused(Refusal::NoAdminCopy.to_string())]);
    assert!(!host.calls.iter().any(|c| c.starts_with("set_up")));
    assert!(!host.held, "given back");
}

#[test]
fn a_pool_user_is_refused_before_any_profile_token_or_folder_is_touched() {
    // The worst case: a pool user signed in to its own session, which it is the user of, on a
    // per-machine install whose task has no deny yet. The gate alone refuses it.
    let mut host = FakeHost::new();
    host.sessions.push((3, sid(POOL)));
    host.profiles
        .push((sid(POOL), PathBuf::from(r"C:\Users\ClauseoSb01")));
    let pool = FakeClient {
        facts: Ok(ClientFacts {
            session: 3,
            ..client(POOL)
        }),
        ..FakeClient::asking(POOL, SET_UP)
    };
    // A member of the group whose SID PoolSids doesn't list yet (a set-up in progress).
    let mut member = client(OTHER);
    member.session = 2;
    member.groups.push(sid(GROUP));
    let member = FakeClient {
        facts: Ok(member),
        ..FakeClient::asking(OTHER, SET_UP)
    };
    let (_, replies, _) = serve_all(&mut host, vec![pool, member]);
    assert_eq!(
        replies,
        [
            Reply::Refused(Refusal::PoolUser.to_string()),
            Reply::Refused(Refusal::PoolGroup.to_string()),
        ]
    );
    assert!(host.acted().is_empty(), "{:?}", host.calls);
    for (error, line) in &host.lines {
        assert!(*error, "{line}");
        assert!(!line.contains("S-1-5"), "no SID in the log: {line}");
    }
}

#[test]
fn only_the_identified_users_profile_is_read_and_its_folder_is_resolved_as_them() {
    // Another user's process in the lawyer's session: refused, no profile read at all.
    let mut host = FakeHost::new();
    let (_, replies, _) = serve_all(&mut host, vec![FakeClient::asking(OTHER, SET_UP)]);
    assert_eq!(
        replies,
        [Reply::Refused(Refusal::NotSessionUser.to_string())]
    );
    assert!(host.acted().is_empty(), "{:?}", host.calls);
    // In their own session: their profile alone, their folder resolved as them.
    let mut host = FakeHost::new();
    let other = FakeClient {
        facts: Ok(ClientFacts {
            session: 2,
            ..client(OTHER)
        }),
        ..FakeClient::asking(OTHER, SET_UP)
    };
    serve_all(&mut host, vec![other]);
    let profiles: Vec<&String> = host
        .calls
        .iter()
        .filter(|c| c.starts_with("profile"))
        .collect();
    assert_eq!(profiles, [&format!("profile {OTHER}")]);
    let resolved: Vec<&String> = host
        .calls
        .iter()
        .filter(|c| c.starts_with("data_dir"))
        .collect();
    let expected = Path::new(r"C:\Users\other")
        .join("AppData")
        .join("Local")
        .join("ai.clauseo.desktop");
    assert_eq!(
        resolved,
        [&format!("data_dir as {OTHER} {}", expected.display())]
    );
    // The owner is the identified user in the client's own session.
    assert!(
        host.calls.contains(&format!("owner 2 {OTHER}")),
        "{:?}",
        host.calls
    );
    // A folder that isn't theirs (a link, another owner) stops before the lock, with its exit.
    let mut host = FakeHost::new();
    host.data_dir_fails = Some((
        SetupExit::DataDirNotOwned,
        "Administrators own it".to_owned(),
    ));
    let (_, replies, _) = serve_all(&mut host, vec![FakeClient::asking(LAWYER, SET_UP)]);
    let [Reply::Ran(result)] = &replies[..] else {
        panic!("{replies:?}")
    };
    assert_eq!(result.exit, SetupExit::DataDirNotOwned.as_code());
    assert!(!host.calls.iter().any(|c| c == "lock"), "{:?}", host.calls);
    // No profile: exit 1, nothing resolved.
    let mut host = FakeHost::new();
    host.profiles.clear();
    let (_, replies, _) = serve_all(&mut host, vec![FakeClient::asking(LAWYER, SET_UP)]);
    let [Reply::Ran(result)] = &replies[..] else {
        panic!("{replies:?}")
    };
    assert_eq!(result.exit, SetupExit::Usage.as_code());
    assert!(
        !host.calls.iter().any(|c| c.starts_with("data_dir")),
        "{:?}",
        host.calls
    );
}

#[test]
fn a_failed_set_up_is_the_reply_and_an_error_line() {
    let mut host = FakeHost::new();
    host.result = SetupResult::failed(SetupExit::OtherOwner, "checking who owns the set-up", "D5");
    let (_, replies, _) = serve_all(&mut host, vec![FakeClient::asking(LAWYER, SET_UP)]);
    assert_eq!(replies, [Reply::Ran(host.result.clone())]);
    assert_eq!(
        host.lines,
        [(
            true,
            "set-up request (set_up): failed (exit 2) at checking who owns the set-up".to_owned()
        )]
    );
}

#[test]
fn only_system_from_the_admin_copy_with_a_recorded_copy_makes_the_pipe() {
    let opened = |host: &mut FakeHost, open: Result<(), Unmade>| {
        let mut made = false;
        let exit = run(host, |_name, _sddl| {
            made = true;
            open.map(|()| FakePipe::default())
        });
        (exit, made)
    };
    let mut host = FakeHost::new();
    host.system = false;
    assert_eq!(opened(&mut host, Ok(())), (ServeExit::NotAllowed, false));
    let mut host = FakeHost::new();
    host.exe = PathBuf::from(r"C:\Users\lawyer\AppData\Local\Clauseo\clauseo-sandbox-setup.exe");
    assert_eq!(opened(&mut host, Ok(())), (ServeExit::NotAllowed, false));
    let mut host = FakeHost::new();
    host.gate = Ok(Gate::default());
    assert_eq!(opened(&mut host, Ok(())), (ServeExit::NoRecord, false));
    let mut host = FakeHost::new();
    host.gate = Err("RegGetValueW failed".to_owned());
    assert_eq!(opened(&mut host, Ok(())), (ServeExit::NoRecord, false));
    // A name someone made first: logged, no one served.
    let mut host = FakeHost::new();
    assert_eq!(
        opened(&mut host, Err(Unmade::Taken)),
        (ServeExit::Squatted, true)
    );
    assert!(
        host.lines
            .iter()
            .any(|(error, line)| *error && line.contains("holds the pipe"))
    );
    let mut host = FakeHost::new();
    let failed = Unmade::Failed("CreateNamedPipeW failed".to_owned());
    assert_eq!(
        opened(&mut host, Err(failed)),
        (ServeExit::PipeFailed, true)
    );
    // A wait that fails ends the run; no client: done at the first wait.
    let mut host = FakeHost::new();
    let mut pipe = FakePipe {
        fail_next: true,
        ..FakePipe::default()
    };
    assert_eq!(run_on(&mut host, &mut pipe).0, ServeExit::PipeFailed);
    // The name taken while the spare was made again: a squat, not a failed pipe.
    let mut pipe = FakePipe {
        taken_next: true,
        ..FakePipe::default()
    };
    assert_eq!(run_on(&mut host, &mut pipe).0, ServeExit::Squatted);
    let mut pipe = FakePipe::default();
    let (exit, sddl) = run_on(&mut FakeHost::new(), &mut pipe);
    assert_eq!(exit, ServeExit::Done);
    assert_eq!(pipe.waits, [FIRST_WAIT]);
    assert_eq!(
        sddl,
        pipe_sddl(&machine_gate()),
        "the pipe's DACL is built from the record"
    );
    assert_eq!(
        [
            ServeExit::Done,
            ServeExit::NotAllowed,
            ServeExit::Squatted,
            ServeExit::NoRecord,
            ServeExit::PipeFailed
        ]
        .map(ServeExit::as_code),
        [0, 63, 64, 65, 66]
    );
}

#[test]
fn requests_and_replies_round_trip_and_nothing_else_parses() {
    assert_eq!(Op::SetUp.to_bytes().unwrap(), SET_UP.as_bytes());
    for op in [Op::SetUp, Op::Add(4), Op::EnsureSlots(3)] {
        assert_eq!(Op::parse(&op.to_bytes().unwrap()), Ok(op));
    }
    assert_eq!(Op::Unsupported("x".to_owned()).to_bytes(), None);
    assert!(Op::parse(&[b' '; REQUEST_CAP + 1]).is_err());
    // Another version, or an op this server doesn't know: unsupported, never malformed.
    for (later, what) in [
        (r#"{"v":2,"op":"set_up","slots":[1]}"#, "protocol 2"),
        (r#"{"v":1,"op":"repair","deep":true}"#, "the op \"repair\""),
    ] {
        assert_eq!(
            Op::parse(later.as_bytes()),
            Ok(Op::Unsupported(what.to_owned()))
        );
    }
    let replies = [
        done(),
        Reply::Refused("no".to_owned()),
        Reply::Unsupported("the op \"ensure_slots\"".to_owned()),
    ];
    for reply in replies {
        let line = reply.to_line();
        assert_eq!(
            line.split(|b| *b == b'\n').count(),
            2,
            "one newline, at the end"
        );
        assert_eq!(Reply::parse(&line), Ok(reply));
    }
    assert_eq!(
        String::from_utf8(done().to_line()).unwrap(),
        "{\"v\":1,\"reply\":\"ran\",\"result\":{\"exit\":0}}\n"
    );
    // A later reply's extra fields are ignored; an unknown tag or version isn't a reply.
    let later = br#"{"v":1,"reply":"refused","why":"x","slots":[]}"#;
    assert_eq!(Reply::parse(later), Ok(Reply::Refused("x".to_owned())));
    for bad in [
        &br#"{"v":1}"#[..],
        br#"{"v":2,"reply":"refused","why":"x"}"#,
        br#"{"v":1,"reply":"slots","list":[]}"#,
        br#"{"v":1,"reply":"refused"}"#,
    ] {
        assert!(
            Reply::parse(bad).is_err(),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
}

#[test]
fn the_log_names_the_op_by_its_tag_and_never_echoes_the_client() {
    let mut host = FakeHost::new();
    let clients = vec![
        FakeClient::asking(LAWYER, r#"{"v":1,"op":"EVIL-op"}"#),
        FakeClient::asking(LAWYER, r#"{"v":1,"op":"set_up","EVIL-field":1}"#),
        FakeClient::asking(LAWYER, "EVIL not json"),
        FakeClient::asking(LAWYER, r#"{"v":1,"op":"add","n":2}"#),
    ];
    let (_, replies, _) = serve_all(&mut host, clients);
    assert_eq!(replies.len(), 4);
    let lines: Vec<&str> = host.lines.iter().map(|(_, line)| line.as_str()).collect();
    assert_eq!(
        lines,
        [
            "set-up request (unsupported) unsupported",
            "set-up request refused: a request this server can't read",
            "set-up request refused: a request this server can't read",
            "set-up request (add): done",
        ]
    );
    // The client still learns why, in its own reply.
    assert!(matches!(&replies[1], Reply::Refused(why) if why.contains("EVIL-field")));
}

#[test]
fn the_app_waits_for_a_reply_as_long_as_the_task_may_run() {
    assert!(REPLY_WAIT > RUN_LIMIT, "past the limit");
    assert!(
        READ_WAIT <= Duration::from_secs(2),
        "a silent client holds no one up"
    );
}

#[test]
fn unsupported_ops_are_answered_once_the_client_is_identified() {
    for (request, what) in [
        (
            r#"{"v":1,"op":"ensure_slots","want":2}"#,
            "the op \"ensure_slots\"",
        ),
        (r#"{"v":1,"op":"frobnicate"}"#, "the op \"frobnicate\""),
        (r#"{"v":7,"op":"set_up"}"#, "protocol 7"),
    ] {
        let mut host = FakeHost::new();
        let (exit, replies, _) = serve_all(&mut host, vec![FakeClient::asking(LAWYER, request)]);
        assert_eq!(exit, ServeExit::Done, "{request}");
        assert_eq!(replies, [Reply::Unsupported(what.to_owned())], "{request}");
        assert!(host.acted().is_empty(), "{request}: {:?}", host.calls);
        assert!(
            host.calls.contains(&"gate".to_owned()),
            "identified first: {:?}",
            host.calls
        );
        // A pool user asking the same is refused as a pool user: identity comes before any op.
        let mut host = FakeHost::new();
        let pool = FakeClient {
            facts: Ok(client(POOL)),
            ..FakeClient::asking(POOL, request)
        };
        let (_, replies, _) = serve_all(&mut host, vec![pool]);
        assert_eq!(
            replies,
            [Reply::Refused(Refusal::PoolUser.to_string())],
            "{request}"
        );
    }
}

#[test]
fn the_data_folder_suffix_is_the_apps_identifier() {
    let config = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../desktop/tauri.conf.json"),
    )
    .unwrap();
    let config: serde_json::Value = serde_json::from_str(&config).unwrap();
    assert_eq!(config["identifier"].as_str(), Some(DATA_SUFFIX[2]));
    assert_eq!(
        DATA_SUFFIX[..2],
        ["AppData", "Local"],
        "the app's $LOCALDATA"
    );
    let profile = Path::new(r"C:\Users\lawyer");
    let dir = data_dir_in(profile).unwrap();
    assert!(dir.starts_with(profile));
    assert!(dir.ends_with("ai.clauseo.desktop"));
}

#[test]
fn the_app_asks_the_task_only_for_the_folder_it_would_set_up() {
    let profile = Path::new(r"C:\Users\lawyer");
    let apps = profile.join(r"AppData\Local\ai.clauseo.desktop");
    assert_eq!(task_serves(Some(profile), &apps), Ok(()));
    let folded = PathBuf::from(r"c:\users\LAWYER\appdata\local\AI.CLAUSEO.DESKTOP");
    assert_eq!(task_serves(Some(profile), &folded), Ok(()), "case folded");
    // A moved AppData, a portable data folder, another user's, or no profile: the prompt.
    for elsewhere in [
        r"D:\Profiles\lawyer\AppData\Local\ai.clauseo.desktop",
        r"C:\Users\lawyer\Documents\Clauseo",
        r"C:\Users\other\AppData\Local\ai.clauseo.desktop",
    ] {
        let why = task_serves(Some(profile), Path::new(elsewhere));
        assert!(why.is_err(), "{elsewhere}");
    }
    assert!(task_serves(None, &apps).is_err());
}
