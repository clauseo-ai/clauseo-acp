use std::net::Ipv4Addr;

use super::{LOCAL, NON_ASCII, UNCLEAR, check, parse_ipv4};

#[test]
fn public_hosts_pass() {
    for url in [
        "https://sci.gov.in/judgments",
        "http://example.com:8080/x?y#z",
        "HTTPS://Example.COM",
        "sci.gov.in",
        "example.com/path@127.0.0.1",
        "https://user:pw@example.com/",
        "https://8.8.8.8/",
        "https://1.example.com/",
        "https://example.com./",
        "https://xn--h2brj9c.xn--h2brj9c/",
        "https://[2001:db8::1]:443/",
        "https://[::ffff:8.8.8.8]/",
        "https://172.32.0.1/",
        "https://192.169.0.1/",
        "https://my_host.example/",
        "about:blank",
        "https://example.com/बाबा",
    ] {
        assert_eq!(check(url), Ok(()), "{url}");
    }
}

#[test]
fn loopback_private_and_link_local_hosts_are_refused() {
    for url in [
        "http://localhost/",
        "http://LOCALHOST:9222/json",
        "localhost",
        "http://localhost./",
        "http://localhost../",
        "http://a.b.localhost/",
        "http://127.0.0.1:41000/api/command",
        "127.0.0.1",
        "http://127.1/",
        "http://127.255.255.254/",
        "http://0.0.0.0/",
        "http://0/",
        "http://2130706433/",
        "2130706433",
        "http://0x7f.1/",
        "http://0x7f000001/",
        "http://0177.0.0.1/",
        "http://017700000001/",
        "http://127.0.0.1./",
        "http://10.1.2.3/",
        "http://172.16.0.1/",
        "http://172.31.255.255/",
        "http://192.168.1.1/",
        "http://169.254.169.254/latest/meta-data",
        "http://[::1]/",
        "http://[::1]:9222/",
        "http://[::]/",
        "http://[0:0:0:0:0:0:0:1]/",
        "http://[fc00::1]/",
        "http://[fd12:3456::1]/",
        "http://[fe80::1]/",
        "http://[febf::1]/",
        "http://[::ffff:127.0.0.1]/",
        "http://[::ffff:7f00:1]/",
        "http://[::ffff:10.0.0.1]/",
        "http://[::127.0.0.1]/",
        "http://evil.example@127.0.0.1/",
        "http://a@b@127.0.0.1/",
        "user@localhost",
        "https://127.0.0.1\\@evil.example/",
    ] {
        assert_eq!(check(url), Err(LOCAL), "{url}");
    }
}

#[test]
fn hosts_the_guard_cannot_settle_are_refused() {
    for url in [
        "",
        "https://",
        "https:///127.0.0.1/",
        "//127.0.0.1",
        "http://127.0.0\n.1/",
        "http://127.0.0\t.1/",
        "http://127.0.0.1 /",
        "http://%31%32%37.0.0.1/",
        "http://127.0.0.1:80:80/",
        "http://127.0.0.1:x/",
        "http://[::1/",
        "http://[::1]x/",
        "http://[fe80::1%25eth0]/",
        "http://[not-ip]/",
        "http://256.0.0.1/",
        "http://1.2.3.4.5/",
        "http://0x100.0.0.1/",
        "http://4294967296/",
        "http://08.0.0.1/",
        "http://1..1/",
        "http://.../",
        "http://exa<mple.com/",
        "http://99999999999999999999999999999999999/",
    ] {
        assert_eq!(check(url), Err(UNCLEAR), "{url:?}");
    }
    for url in [
        "http://１２７.０.０.１/",
        "https://\u{212A}.example/",
        "http://bücher.de/",
    ] {
        assert_eq!(check(url), Err(NON_ASCII), "{url}");
    }
}

#[test]
fn ipv4_reads_as_whatwg_reads_it() {
    for (text, address) in [
        ("127.0.0.1", Ipv4Addr::LOCALHOST),
        ("127.1", Ipv4Addr::LOCALHOST),
        ("127.0.1", Ipv4Addr::LOCALHOST),
        ("2130706433", Ipv4Addr::LOCALHOST),
        ("0x7f.1", Ipv4Addr::LOCALHOST),
        ("0x7F000001", Ipv4Addr::LOCALHOST),
        ("0177.0.0.01", Ipv4Addr::LOCALHOST),
        ("0x.0x.0", Ipv4Addr::UNSPECIFIED),
        ("1.2.65535", Ipv4Addr::new(1, 2, 255, 255)),
    ] {
        assert_eq!(parse_ipv4(text), Some(address), "{text}");
    }
    for bad in [
        "",
        "1.",
        "1.2.3.4.5",
        "256.1",
        "1.2.65536",
        "4294967296",
        "09",
        "0xg",
    ] {
        assert_eq!(parse_ipv4(bad), None, "{bad}");
    }
}
