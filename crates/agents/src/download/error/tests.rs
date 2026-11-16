use std::io;

use super::*;

#[test]
fn the_classifier_sorts_every_branch() {
    use ureq::Error as E;
    let io = |kind| E::Io(io::Error::from(kind));
    let offline = [
        E::HostNotFound,
        E::Timeout(ureq::Timeout::Connect),
        E::Timeout(ureq::Timeout::Resolve),
        io(io::ErrorKind::NetworkUnreachable),
        io(io::ErrorKind::HostUnreachable),
        io(io::ErrorKind::NetworkDown),
        io(io::ErrorKind::ConnectionAborted),
        io(io::ErrorKind::ConnectionReset),
        io(io::ErrorKind::TimedOut),
    ];
    for err in &offline {
        assert_eq!(classify(err), FetchKind::Offline, "{err:?}");
    }
    let rustls_io = io::Error::new(
        io::ErrorKind::InvalidData,
        rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer),
    );
    let blocked = [
        E::Io(rustls_io),
        E::Tls("handshake"),
        E::Rustls(rustls::Error::General("bad".to_owned())),
        E::ConnectProxyFailed("proxy server responded 407/Proxy Authentication Required".into()),
        E::ConnectProxyFailed("proxy server responded 403/Forbidden".into()),
    ];
    for err in &blocked {
        assert_eq!(classify(err), FetchKind::Blocked, "{err:?}");
    }
    let failed = [
        io(io::ErrorKind::ConnectionRefused),
        io(io::ErrorKind::UnexpectedEof),
        E::Timeout(ureq::Timeout::RecvBody),
        E::ConnectProxyFailed("proxy server did not respond".into()),
        E::ConnectProxyFailed("proxy server responded 502/Bad Gateway".into()),
        E::TooManyRedirects,
    ];
    for err in &failed {
        assert_eq!(classify(err), FetchKind::Failed, "{err:?}");
    }
    let proxy = E::ConnectProxyFailed("proxy server responded 407/Proxy Auth".into());
    assert_eq!(error_status(&proxy), Some(407));
    assert_eq!(error_status(&E::HostNotFound), None);

    assert_eq!(
        write_kind(&io::Error::from(io::ErrorKind::StorageFull)),
        FetchKind::DiskFull
    );
    assert_eq!(
        write_kind(&io::Error::from(io::ErrorKind::PermissionDenied)),
        FetchKind::Failed
    );
    assert_eq!(status_kind(403), FetchKind::Blocked);
    assert_eq!(status_kind(407), FetchKind::Blocked);
    assert_eq!(status_kind(404), FetchKind::Failed);
    assert_eq!(status_kind(503), FetchKind::Failed);
}

#[test]
fn the_host_drops_user_info() {
    let host = host_of("https://u:p@mirror.test:8443/a?b");
    assert_eq!(host.as_deref(), Some("mirror.test:8443"));
    let host = host_of("https://registry.npmjs.org/x");
    assert_eq!(host.as_deref(), Some("registry.npmjs.org"));
    assert_eq!(host_of("not a url"), None);
}

/// Each kind of a network gone away is offline, on every OS, and so is std's Unix lookup
/// failure and an io error wrapping one of them.
#[test]
fn each_lost_network_kind_is_offline_and_a_wrapped_one_too() {
    for kind in [
        io::ErrorKind::NetworkUnreachable,
        io::ErrorKind::HostUnreachable,
        io::ErrorKind::NetworkDown,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::TimedOut,
    ] {
        assert_eq!(
            classify_io(&io::Error::from(kind)),
            FetchKind::Offline,
            "{kind:?}"
        );
        let wrapped = io::Error::other(io::Error::from(kind));
        assert_eq!(
            classify_io(&wrapped),
            FetchKind::Offline,
            "wrapped {kind:?}"
        );
    }
    let lookup = io::Error::other(
        "failed to lookup address information: Temporary failure in name resolution",
    );
    assert_eq!(classify_io(&lookup), FetchKind::Offline);
    for kind in [
        io::ErrorKind::ConnectionRefused,
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::PermissionDenied,
    ] {
        assert_eq!(
            classify_io(&io::Error::from(kind)),
            FetchKind::Failed,
            "{kind:?}"
        );
    }
    assert_eq!(
        classify_io(&io::Error::other("something else")),
        FetchKind::Failed
    );
}

/// Each Windows Sockets code of a network gone away or a failed lookup is offline, one by one,
/// bare and inside ureq's error; `WSAECONNREFUSED` stays failed, as `ECONNREFUSED` does.
#[cfg(windows)]
#[test]
fn each_offline_winsock_code_is_offline() {
    for (code, name) in [
        (10050, "WSAENETDOWN"),
        (10051, "WSAENETUNREACH"),
        (10053, "WSAECONNABORTED"),
        (10054, "WSAECONNRESET"),
        (10060, "WSAETIMEDOUT"),
        (10065, "WSAEHOSTUNREACH"),
        (11001, "WSAHOST_NOT_FOUND"),
        (11002, "WSATRY_AGAIN"),
        (11004, "WSANO_DATA"),
    ] {
        assert!(OFFLINE_WSA_CODES.contains(&code), "{name}");
        let err = io::Error::from_raw_os_error(code);
        assert_eq!(classify_io(&err), FetchKind::Offline, "{name}");
        let ureq_err = ureq::Error::Io(io::Error::from_raw_os_error(code));
        assert_eq!(
            classify(&ureq_err),
            FetchKind::Offline,
            "{name} through ureq"
        );
    }
    assert_eq!(
        classify_io(&io::Error::from_raw_os_error(10061)),
        FetchKind::Failed,
        "WSAECONNREFUSED"
    );
}
