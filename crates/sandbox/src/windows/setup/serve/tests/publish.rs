//! The pipe's name for each run, and its publishing: drawn at random, the pipe made under it
//! before it is published, and cleared as the server exits, whichever way.

use super::*;

/// The token the fake's random bytes give.
fn fake_token() -> String {
    token(&[0xa5; TOKEN_BYTES])
}

/// [`run`] over `host` and `pipe`: the exit, and the name the pipe was made under (none when
/// it wasn't made).
fn run_named(host: &mut FakeHost, pipe: FakePipe) -> (ServeExit, Option<String>) {
    let mut opened = None;
    let exit = run(host, |name, _sddl| {
        opened = Some(name.to_owned());
        Ok(pipe)
    });
    (exit, opened)
}

#[test]
fn a_run_token_is_128_random_bits_in_hex_and_names_only_its_own_pipe() {
    let token = fake_token();
    assert_eq!(token, "a5".repeat(16));
    let name = pipe_name(&token).unwrap();
    assert_eq!(name, format!(r"\\.\pipe\ClauseoSetUp-{token}"));
    // Each draw its own name.
    let mut other = [0xa5; TOKEN_BYTES];
    other[15] = 0xa4;
    assert_ne!(pipe_name(&super::token(&other)), Some(name));
    let zeros = super::token(&[0; TOKEN_BYTES]);
    assert_eq!(pipe_name(&zeros).unwrap().len(), PIPE_PREFIX.len() + 32);
    // A published value that isn't exactly 32 lowercase hex digits names nothing.
    for value in [
        "",
        "a5",
        &"A5".repeat(16),
        &"a5".repeat(17),
        &format!("{}\\..\\x", "a5".repeat(14)),
        &format!("{}zz", "a5".repeat(15)),
    ] {
        assert_eq!(pipe_name(value), None, "{value:?}");
    }
}

#[test]
fn the_pipe_is_made_under_the_runs_name_then_published_then_cleared_as_it_exits() {
    let mut host = FakeHost::new();
    let pipe = FakePipe {
        waiting: vec![FakeClient::asking(LAWYER, SET_UP)].into(),
        ..FakePipe::default()
    };
    let (exit, opened) = run_named(&mut host, pipe);
    assert_eq!(exit, ServeExit::Done);
    let token = fake_token();
    assert_eq!(opened, pipe_name(&token));
    let publish = format!("publish {token}");
    let unpublish = format!("unpublish {token}");
    let at = |call: &str| host.calls.iter().position(|made| made == call).unwrap();
    assert!(at("random") < at(&publish), "{:?}", host.calls);
    // Published before the first client was served, cleared after the last.
    assert!(at(&publish) < at("lock"), "{:?}", host.calls);
    assert_eq!(host.calls.last(), Some(&unpublish));
    assert_eq!(host.published, None, "cleared at exit");
}

#[test]
fn a_name_is_published_only_for_a_pipe_this_run_made_and_cleared_on_every_exit_after() {
    // The pipe wasn't made (its name taken, which a random name can't be in practice): nothing
    // published.
    let mut host = FakeHost::new();
    let exit = run(&mut host, |_name, _sddl| Err::<FakePipe, _>(Unmade::Taken));
    assert_eq!(exit, ServeExit::Squatted);
    assert!(
        host.calls.iter().all(|call| !call.contains("publish")),
        "{:?}",
        host.calls
    );
    // No random bytes: no pipe, nothing published.
    let mut host = FakeHost::new();
    host.random = Err("BCryptGenRandom failed".to_owned());
    assert_eq!(
        run_named(&mut host, FakePipe::default()),
        (ServeExit::Unpublished, None)
    );
    // 69: 67 is the WSL upgrade's, 68 the denial watcher's.
    assert_eq!(ServeExit::Unpublished.as_code(), 69);
    // A name it can't publish: no client is waited for, and what it may have left is cleared.
    let mut host = FakeHost::new();
    host.publish_fails = Some("RegSetValueExW failed (error 5)".to_owned());
    let mut pipe = FakePipe::default();
    let exit = run(&mut host, |_name, _sddl| Ok(&mut pipe));
    assert_eq!(exit, ServeExit::Unpublished);
    assert!(pipe.waits.is_empty(), "no one served: {:?}", pipe.waits);
    assert_eq!(
        host.calls.last(),
        Some(&format!("unpublish {}", fake_token()))
    );
    assert!(
        host.lines
            .iter()
            .any(|(error, line)| *error && line.contains("publishing"))
    );
    // Its name taken while no instance was left, or a failed wait: cleared all the same.
    for pipe in [
        FakePipe {
            taken_next: true,
            ..FakePipe::default()
        },
        FakePipe {
            fail_next: true,
            ..FakePipe::default()
        },
    ] {
        let mut host = FakeHost::new();
        let (exit, _) = run_named(&mut host, pipe);
        assert!(matches!(exit, ServeExit::Squatted | ServeExit::PipeFailed));
        assert_eq!(host.published, None, "{exit:?}");
        assert_eq!(
            host.calls.last(),
            Some(&format!("unpublish {}", fake_token()))
        );
    }
}

impl Pipe for &mut FakePipe {
    type Client = FakeClient;

    fn next(&mut self, wait: Duration) -> Result<Option<FakeClient>, Unmade> {
        (**self).next(wait)
    }

    fn request(&mut self, client: &mut FakeClient) -> Result<Vec<u8>, String> {
        (**self).request(client)
    }

    fn facts(&mut self, client: &mut FakeClient) -> Result<ClientFacts, String> {
        (**self).facts(client)
    }

    fn reply(&mut self, client: FakeClient, line: &[u8]) {
        (**self).reply(client, line);
    }
}
