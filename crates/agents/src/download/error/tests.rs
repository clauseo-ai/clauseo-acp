use std::io;

use super::*;

#[test]
fn the_classifier_sorts_every_branch() {
    use ureq::Error as E;
    let io = |kind| E::Io(io::Error::from(kind));
    let offline = [
        E::HostNotFound,
        E::Timeout(ureq::Timeout::Resolve),
        io(io::ErrorKind::NetworkUnreachable),
        io(io::ErrorKind::HostUnreachable),
        io(io::ErrorKind::NetworkDown),
    ];
    for err in &offline {
        assert_eq!(classify(err), FetchKind::Offline, "{err:?}");
    }
    let lost = [
        E::Timeout(ureq::Timeout::Connect),
        E::Timeout(ureq::Timeout::RecvResponse),
        E::Timeout(ureq::Timeout::RecvBody),
        E::Timeout(ureq::Timeout::PerRead),
        io(io::ErrorKind::ConnectionAborted),
        io(io::ErrorKind::ConnectionReset),
        io(io::ErrorKind::TimedOut),
    ];
    for err in &lost {
        assert_eq!(classify(err), FetchKind::Lost, "{err:?}");
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
        E::Timeout(ureq::Timeout::Global),
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

/// Each kind of a network gone away is offline, and each kind of a connection lost is lost, on
/// every OS, bare and wrapped; std's Unix lookup failure is offline.
#[test]
fn each_network_kind_has_its_class_and_a_wrapped_one_too() {
    for (kind, class) in [
        (io::ErrorKind::NetworkUnreachable, FetchKind::Offline),
        (io::ErrorKind::HostUnreachable, FetchKind::Offline),
        (io::ErrorKind::NetworkDown, FetchKind::Offline),
        (io::ErrorKind::ConnectionAborted, FetchKind::Lost),
        (io::ErrorKind::ConnectionReset, FetchKind::Lost),
        (io::ErrorKind::TimedOut, FetchKind::Lost),
    ] {
        assert_eq!(classify_io(&io::Error::from(kind)), class, "{kind:?}");
        let wrapped = io::Error::other(io::Error::from(kind));
        assert_eq!(classify_io(&wrapped), class, "wrapped {kind:?}");
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

/// A body read that timed out is one class however it arrives: a bare io `TimedOut` (a Windows
/// recv's 10060) and ureq's own `Timeout(RecvBody)` are both lost, as a connect timeout is.
#[test]
fn every_timeout_on_the_network_is_lost() {
    let bare = classify(&ureq::Error::Io(io::Error::from(io::ErrorKind::TimedOut)));
    let ureq_body = classify(&ureq::Error::Timeout(ureq::Timeout::RecvBody));
    let ureq_connect = classify(&ureq::Error::Timeout(ureq::Timeout::Connect));
    assert_eq!(bare, FetchKind::Lost);
    assert_eq!(ureq_body, bare);
    assert_eq!(ureq_connect, bare);
}

/// Each Windows Sockets code of a network gone away or a failed lookup has its class, one by
/// one, bare and inside ureq's error: the lookup codes (the list) and the unreachable ones
/// offline, the reset, aborted and timed-out ones lost. `WSAECONNREFUSED` stays failed, as
/// `ECONNREFUSED` does.
#[cfg(windows)]
#[test]
fn each_winsock_code_has_its_class() {
    for (code, name, class) in [
        (10050, "WSAENETDOWN", FetchKind::Offline),
        (10051, "WSAENETUNREACH", FetchKind::Offline),
        (10065, "WSAEHOSTUNREACH", FetchKind::Offline),
        (11001, "WSAHOST_NOT_FOUND", FetchKind::Offline),
        (11002, "WSATRY_AGAIN", FetchKind::Offline),
        (11004, "WSANO_DATA", FetchKind::Offline),
        (10053, "WSAECONNABORTED", FetchKind::Lost),
        (10054, "WSAECONNRESET", FetchKind::Lost),
        (10060, "WSAETIMEDOUT", FetchKind::Lost),
    ] {
        let err = io::Error::from_raw_os_error(code);
        assert_eq!(classify_io(&err), class, "{name}");
        let ureq_err = ureq::Error::Io(io::Error::from_raw_os_error(code));
        assert_eq!(classify(&ureq_err), class, "{name} through ureq");
    }
    assert_eq!(OFFLINE_WSA_CODES, [11001, 11002, 11004]);
    assert_eq!(
        classify_io(&io::Error::from_raw_os_error(10061)),
        FetchKind::Failed,
        "WSAECONNREFUSED"
    );
}
