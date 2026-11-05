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
