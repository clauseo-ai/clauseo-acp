//! Private IP literals and `localhost` names are refused before the allowlist, on both
//! listeners and in both forms, even when the list names them.

use std::io::Write;
use std::net::SocketAddr;

use super::harness::{Rig, host_denial, read_all, read_head, rig_allowing, status};
use crate::HostRefusal::{NotAllowed, PrivateAddress};

/// Each private host as a request writes it, and as its denial names it.
const PRIVATE: &[(&str, &str)] = &[
    ("[::1]", "::1"),
    ("127.0.0.2", "127.0.0.2"),
    ("10.0.0.1", "10.0.0.1"),
    ("0x7f.0.0.2", "127.0.0.2"),
    ("[::ffff:127.0.0.1]", "::ffff:127.0.0.1"),
    ("127.1", "127.0.0.1"),
    ("2130706433", "127.0.0.1"),
    ("0", "0.0.0.0"),
    ("0.0.0.0", "0.0.0.0"),
    ("[::]", "::"),
    ("LOCALHOST.", "localhost"),
    ("localhost", "localhost"),
    ("foo.localhost", "foo.localhost"),
    ("Foo.LocalHost.", "foo.localhost"),
    // Off the list: refused as private, not as unlisted.
    ("192.168.1.1", "192.168.1.1"),
    ("bar.localhost", "bar.localhost"),
];

/// The list names most private hosts above, so only the order of the checks refuses them.
const LISTED: &[&str] = &[
    "[::1]",
    "127.0.0.2",
    "10.0.0.1",
    "[::ffff:127.0.0.1]",
    "localhost",
    "foo.localhost",
];

#[derive(Clone, Copy, Debug)]
enum Via {
    Agent,
    Browser,
}

#[derive(Clone, Copy, Debug)]
enum Form {
    Connect,
    Http,
}

/// Sends one request for `host` and returns the answer's status.
fn ask(rig: &Rig, via: Via, form: Form, host: &str) -> u16 {
    let line = match form {
        Form::Connect => format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n"),
        Form::Http => format!("GET http://{host}/ HTTP/1.1\r\nHost: {host}\r\n"),
    };
    let (mut stream, request) = match via {
        Via::Agent => (rig.agent(), format!("{line}{}\r\n", rig.auth())),
        Via::Browser => (rig.browser(), format!("{line}\r\n")),
    };
    stream.write_all(request.as_bytes()).unwrap();
    let code = status(&read_head(&mut stream));
    drop(read_all(&mut stream));
    code
}

#[test]
fn private_literals_and_localhost_are_refused_before_the_list_everywhere() {
    for via in [Via::Agent, Via::Browser] {
        for form in [Form::Connect, Form::Http] {
            let rig = rig_allowing(LISTED);
            rig.net.name("localhost", &["192.0.2.10"]);
            rig.net.name("foo.localhost", &["192.0.2.10"]);
            let port = match form {
                Form::Connect => 443,
                Form::Http => 80,
            };
            let mut expected = Vec::new();
            for &(host, named) in PRIVATE {
                assert_eq!(ask(&rig, via, form, host), 403, "{via:?} {form:?} {host}");
                expected.push(host_denial(named, port, PrivateAddress, false));
            }
            assert_eq!(rig.denials(), expected, "{via:?} {form:?}");
            assert_eq!(rig.net.resolved(), Vec::<String>::new(), "{via:?} {form:?}");
            assert_eq!(
                rig.net.dialled(),
                Vec::<SocketAddr>::new(),
                "{via:?} {form:?}"
            );
        }
    }
}

#[test]
fn a_public_literal_off_the_list_is_still_not_allowed() {
    for via in [Via::Agent, Via::Browser] {
        for form in [Form::Connect, Form::Http] {
            let rig = rig_allowing(LISTED);
            let port = match form {
                Form::Connect => 443,
                Form::Http => 80,
            };
            assert_eq!(ask(&rig, via, form, "192.0.2.11"), 403, "{via:?} {form:?}");
            assert_eq!(
                ask(&rig, via, form, "[2001:db8::1]"),
                403,
                "{via:?} {form:?}"
            );
            assert_eq!(
                rig.denials(),
                vec![
                    host_denial("192.0.2.11", port, NotAllowed, false),
                    host_denial("2001:db8::1", port, NotAllowed, false),
                ],
                "{via:?} {form:?}"
            );
            assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
        }
    }
}

#[test]
fn a_name_that_only_ends_in_localhost_is_not_refused_as_private() {
    let rig = rig_allowing(&["api.anthropic.com"]);
    for host in ["notlocalhost", "localhost.example"] {
        assert_eq!(ask(&rig, Via::Agent, Form::Connect, host), 403, "{host}");
    }
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("notlocalhost", 443, NotAllowed, false),
            host_denial("localhost.example", 443, NotAllowed, false),
        ]
    );
}

#[test]
fn an_ipv6_literal_with_a_zone_is_a_bad_request_with_no_denial() {
    for via in [Via::Agent, Via::Browser] {
        for form in [Form::Connect, Form::Http] {
            let rig = rig_allowing(LISTED);
            assert_eq!(ask(&rig, via, form, "[::1%25lo0]"), 400, "{via:?} {form:?}");
            assert_eq!(rig.denials(), Vec::new(), "{via:?} {form:?}");
            assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
        }
    }
}
