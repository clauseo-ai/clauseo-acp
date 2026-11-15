//! Open mode's address checks, on the browser listener it opens: a name that resolves to any
//! address that is not public is refused `PrivateAddress` before anything is dialled, over
//! CONNECT and plain http, and an IP literal in any of those ranges is refused before DNS. Any
//! public host passing must not open the way to loopback, the private ranges or cloud metadata.

use std::io::Write as _;
use std::net::SocketAddr;

use super::harness::{Rig, Setup, echo, host_denial, origin, read_all, read_head, rig, status};
use crate::HostRefusal::PrivateAddress;
use crate::proxy::ProxyMode;

/// Answers a name may get, each alone, every one refused.
const PRIVATE_ANSWERS: &[&[&str]] = &[
    &["127.0.0.1"],
    &["::1"],
    &["10.1.2.3"],
    &["192.168.1.1"],
    &["172.16.0.1"],
    &["100.64.0.1"],
    &["169.254.169.254"],
    &["fc00::1"],
    &["fd12:3456::1"],
    &["fe80::1"],
    &["ff02::1"],
    &["224.0.0.1"],
    &["0.0.0.0"],
    &["::"],
    &["::ffff:127.0.0.1"],
    // One public answer does not carry a private one, in either order.
    &["192.0.2.10", "10.0.0.1"],
    &["::1", "192.0.2.10"],
];

/// Private IP literals as a request writes them, and as the denial names them.
const PRIVATE_LITERALS: &[(&str, &str)] = &[
    ("127.0.0.1", "127.0.0.1"),
    ("[::1]", "::1"),
    ("10.1.2.3", "10.1.2.3"),
    ("192.168.1.1", "192.168.1.1"),
    ("172.16.0.1", "172.16.0.1"),
    ("100.64.0.1", "100.64.0.1"),
    ("169.254.169.254", "169.254.169.254"),
    ("[fc00::1]", "fc00::1"),
    ("[fd12:3456::1]", "fd12:3456::1"),
    ("[fe80::1]", "fe80::1"),
    ("[ff02::1]", "ff02::1"),
    ("224.0.0.1", "224.0.0.1"),
    ("0.0.0.0", "0.0.0.0"),
    ("[::]", "::"),
    ("[::ffff:127.0.0.1]", "::ffff:127.0.0.1"),
    ("localhost", "localhost"),
];

#[derive(Clone, Copy, Debug)]
enum Form {
    Connect,
    Http,
}

impl Form {
    fn port(self) -> u16 {
        match self {
            Form::Connect => 443,
            Form::Http => 80,
        }
    }
}

/// An open-mode rig with no list at all, where every address the tests use has a server, so
/// only the address check can refuse.
fn open_rig() -> Rig {
    let rig = rig(Setup {
        mode: ProxyMode::Open,
        ..Setup::default()
    });
    let (http, _received) = origin("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    let tls = echo();
    for answers in PRIVATE_ANSWERS {
        for ip in *answers {
            let ip: std::net::IpAddr = ip.parse().unwrap();
            rig.net.route(&SocketAddr::new(ip, 443).to_string(), tls);
            rig.net.route(&SocketAddr::new(ip, 80).to_string(), http);
        }
    }
    rig
}

/// Sends one request for `host` on the browser listener; the answer's status.
fn ask(rig: &Rig, form: Form, host: &str) -> u16 {
    let line = match form {
        Form::Connect => format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n"),
        Form::Http => format!("GET http://{host}/ HTTP/1.1\r\nHost: {host}\r\n"),
    };
    let mut stream = rig.browser();
    stream
        .write_all(format!("{line}{}\r\n", rig.browser_auth()).as_bytes())
        .unwrap();
    let code = status(&read_head(&mut stream));
    drop(read_all(&mut stream));
    code
}

#[test]
fn open_mode_refuses_a_name_with_any_private_answer_before_any_dial() {
    for form in [Form::Connect, Form::Http] {
        let rig = open_rig();
        let mut expected = Vec::new();
        let mut asked = Vec::new();
        for (i, answers) in PRIVATE_ANSWERS.iter().enumerate() {
            let host = format!("inside{i}.test");
            rig.net.name(&host, answers);
            assert_eq!(ask(&rig, form, &host), 403, "{form:?} {answers:?}");
            expected.push(host_denial(&host, form.port(), PrivateAddress, false));
            asked.push(host);
        }
        assert_eq!(rig.denials(), expected, "{form:?}");
        // Each name resolved once, and nothing dialled.
        assert_eq!(rig.net.resolved(), asked, "{form:?}");
        assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new(), "{form:?}");
    }
}

#[test]
fn open_mode_refuses_a_private_literal_before_dns() {
    for form in [Form::Connect, Form::Http] {
        let rig = open_rig();
        let mut expected = Vec::new();
        for &(host, named) in PRIVATE_LITERALS {
            assert_eq!(ask(&rig, form, host), 403, "{form:?} {host}");
            expected.push(host_denial(named, form.port(), PrivateAddress, false));
        }
        assert_eq!(rig.denials(), expected, "{form:?}");
        assert_eq!(rig.net.resolved(), Vec::<String>::new(), "{form:?}");
        assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new(), "{form:?}");
    }
}
