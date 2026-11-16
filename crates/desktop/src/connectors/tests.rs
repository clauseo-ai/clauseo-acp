use clauseo_connectors::ProxyChoice;
use clauseo_protocol::HostError;
use clauseo_sandbox::Route;

use super::{open_link, proxy_for};
use crate::open::Opener;

/// A firm proxy becomes a `Fixed` one, an IPv6 literal in brackets; its `Debug` names the
/// scheme, host and port only.
#[test]
fn a_route_becomes_the_proxy_the_connector_takes() {
    assert_eq!(proxy_for(Route::Direct), Ok(ProxyChoice::Direct));
    let http = proxy_for(Route::Http {
        host: "proxy.firm.example".to_owned(),
        port: 3128,
    })
    .unwrap();
    assert_eq!(format!("{http:?}"), "Fixed(http://proxy.firm.example:3128)");
    let v6 = proxy_for(Route::Http {
        host: "fd00::1".to_owned(),
        port: 8080,
    })
    .unwrap();
    assert_eq!(format!("{v6:?}"), "Fixed(http://[fd00::1]:8080)");
}

/// An opener that records what it opened.
#[derive(Default)]
struct Recorder(std::sync::Mutex<Vec<String>>);

impl Opener for Recorder {
    fn open_url(&self, url: &str) -> Result<(), HostError> {
        self.0.lock().unwrap().push(url.to_owned());
        Ok(())
    }
    fn open_path(&self, _path: &std::path::Path, _kind: &str) -> Result<(), HostError> {
        unreachable!("the connectors open links only")
    }
    fn reveal(&self, _path: &std::path::Path) -> Result<(), HostError> {
        unreachable!("the connectors open links only")
    }
}

/// The sign-in page opens with no confirm when it is https; anything else is refused with an
/// error that names no URL.
#[test]
fn the_sign_in_page_opens_https_only_and_errors_name_no_url() {
    let opener = Recorder::default();
    let page = "https://data.clauseo.chat/oauth/authorize?state=s3cr3t";
    open_link(&opener, page).unwrap();
    let plain = "http://data.clauseo.chat/oauth/authorize?state=s3cr3t";
    let error = open_link(&opener, plain).unwrap_err();
    assert!(
        !error.contains("s3cr3t") && !error.contains("data.clauseo"),
        "{error}"
    );
    assert_eq!(*opener.0.lock().unwrap(), [page]);
}

/// A keychain that holds nothing.
struct NoKeychain;

impl clauseo_connectors::SecretStore for NoKeychain {
    fn get(&self, _account: &str) -> Result<Option<Vec<u8>>, clauseo_connectors::StoreError> {
        Ok(None)
    }
    fn set(&self, _account: &str, _secret: &[u8]) -> Result<(), clauseo_connectors::StoreError> {
        Ok(())
    }
    fn delete(&self, _account: &str) -> Result<(), clauseo_connectors::StoreError> {
        Ok(())
    }
}

/// An opener the start never uses.
struct NoOpener;

impl clauseo_host::connectors::ConnectorOpener for NoOpener {
    fn open(&self, _url: &str) -> Result<(), String> {
        Err("not in this test".to_owned())
    }
    fn open_confirmed(&self, _url: &str) -> Result<(), HostError> {
        Err(HostError::new(
            clauseo_protocol::ErrorCode::Internal,
            "not in this test",
        ))
    }
}

/// Answers every confirm with `answer` and counts them.
struct Confirm {
    answer: bool,
    asked: std::sync::atomic::AtomicUsize,
}

impl crate::open::LinkConfirm for Confirm {
    fn confirm_link(&self, _words: crate::open::LinkWords) -> futures::future::BoxFuture<'_, bool> {
        self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let answer = self.answer;
        Box::pin(async move { answer })
    }
}

fn pay(confirm: &Confirm, opener: &Recorder, url: &str) -> Result<(), HostError> {
    let gate = crate::open::gate::Gate::default();
    futures::executor::block_on(super::open_pay_link(
        confirm,
        opener,
        &gate,
        &crate::open::now,
        clauseo_protocol::Language::En,
        url,
    ))
}

/// `OpenPayLink`'s open asks the native confirm first: accepted, the link opens; declined,
/// nothing opens (`transport.unauthorized`); a link that is not https is refused before any
/// confirm.
#[test]
fn the_pay_link_opens_only_through_the_confirm() {
    use std::sync::atomic::Ordering;
    let link = "https://data.clauseo.chat/pay/rch_123";
    let yes = Confirm {
        answer: true,
        asked: 0.into(),
    };
    let opener = Recorder::default();
    pay(&yes, &opener, link).unwrap();
    assert_eq!(yes.asked.load(Ordering::SeqCst), 1);
    assert_eq!(*opener.0.lock().unwrap(), [link]);

    let no = Confirm {
        answer: false,
        asked: 0.into(),
    };
    let opener = Recorder::default();
    let declined = pay(&no, &opener, link).unwrap_err();
    assert_eq!(
        declined.code(),
        clauseo_protocol::ErrorCode::TransportUnauthorized
    );
    assert!(opener.0.lock().unwrap().is_empty());

    let plain = pay(&yes, &opener, "http://data.clauseo.chat/pay/rch_123").unwrap_err();
    assert_eq!(plain.code(), clauseo_protocol::ErrorCode::CommandInvalid);
    assert_eq!(
        yes.asked.load(Ordering::SeqCst),
        1,
        "no confirm for a refused link"
    );
    assert!(opener.0.lock().unwrap().is_empty());
}

/// The host's start builds the connectors without asking the route: the lookup (up to 10 s on
/// a WPAD script) waits for the first connector request, off the start thread.
#[test]
fn the_start_builds_the_connectors_without_waiting_for_the_route() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use clauseo_connectors::{ProxyRefused, ProxyResolver};

    let dir = std::env::temp_dir().join(format!("clauseo-desktop-route-{}", std::process::id()));
    let asked = Arc::new(AtomicUsize::new(0));
    let route = {
        let asked = Arc::clone(&asked);
        ProxyResolver::new(move || {
            asked.fetch_add(1, Ordering::SeqCst);
            Err(ProxyRefused)
        })
    };
    let built = super::build(&dir, Arc::new(NoOpener), Arc::new(NoKeychain), route);
    assert!(built.is_ok(), "{:?}", built.err());
    assert_eq!(asked.load(Ordering::SeqCst), 0, "the start asked the route");
    assert!(!dir.exists(), "the start wrote to the data folder");
}

/// The connector's origin is the compile-time feature's: production by default, the staging host
/// with `staging-connector`. The connectors the start builds take it, and a staging build says
/// so in its user agent.
#[test]
fn the_origin_comes_from_the_compile_time_feature_alone() {
    use std::sync::Arc;

    use clauseo_connectors::{Origin, ProxyRefused, ProxyResolver};

    #[cfg(all(feature = "staging-connector", not(feature = "local-connector")))]
    let expected = Origin::Staging;
    #[cfg(feature = "local-connector")]
    let expected = Origin::Local;
    #[cfg(not(any(feature = "staging-connector", feature = "local-connector")))]
    let expected = Origin::Production;
    assert_eq!(super::ORIGIN, expected);
    let dir = std::env::temp_dir().join(format!("clauseo-desktop-origin-{}", std::process::id()));
    let route = ProxyResolver::new(|| Err(ProxyRefused));
    let (wiring, _relays) =
        super::build(&dir, Arc::new(NoOpener), Arc::new(NoKeychain), route).unwrap();
    assert_eq!(wiring.connectors.origin(), &expected);
    assert_eq!(
        super::user_agent().ends_with(" (staging)"),
        cfg!(all(
            feature = "staging-connector",
            not(feature = "local-connector")
        )),
        "{}",
        super::user_agent()
    );
    assert_eq!(
        super::user_agent().ends_with(" (local)"),
        cfg!(feature = "local-connector"),
        "{}",
        super::user_agent()
    );
}

/// `OpenPayLink`'s check under this build's origin: a production build takes only the
/// production pay link, a staging build only the staging one, both https only.
#[test]
fn the_pay_link_follows_the_build_s_origin() {
    use clauseo_connectors::{CASE_LAW, PayUrl};

    let production = PayUrl::parse(&CASE_LAW, "https://data.clauseo.chat/pay/rch_1").unwrap();
    #[cfg(not(any(feature = "staging-connector", feature = "local-connector")))]
    assert!(production.is_for(&CASE_LAW, &super::ORIGIN));
    #[cfg(feature = "local-connector")]
    {
        assert!(!production.is_for(&CASE_LAW, &super::ORIGIN));
        let text = "http://127.0.0.1:8793/pay/rch_AB12cd34";
        let local = PayUrl::parse_for(&CASE_LAW, &super::ORIGIN, text).unwrap();
        assert!(local.is_for(&CASE_LAW, &super::ORIGIN));
        assert!(!local.is_for(&CASE_LAW, &clauseo_connectors::Origin::Production));
    }
    #[cfg(all(feature = "staging-connector", not(feature = "local-connector")))]
    {
        use clauseo_connectors::Origin;

        assert!(!production.is_for(&CASE_LAW, &super::ORIGIN));
        let staging = PayUrl::parse_for(
            &CASE_LAW,
            &Origin::Staging,
            "https://data-staging.clauseo.chat/pay/rch_1",
        )
        .unwrap();
        assert!(staging.is_for(&CASE_LAW, &super::ORIGIN));
        assert!(!staging.is_for(&CASE_LAW, &Origin::Production));
    }
    for text in [
        "http://data.clauseo.chat/pay/rch_1",
        "http://data-staging.clauseo.chat/pay/rch_1",
        "https://evil.example/pay/rch_1",
    ] {
        assert_eq!(
            PayUrl::parse_for(&CASE_LAW, &super::ORIGIN, text),
            None,
            "{text}"
        );
    }
}

/// No setting, policy, environment variable or argument reaches the origin: the module that
/// sets it reads none of them and sets it from `ORIGIN` only, and no other file of the app
/// names a connector origin. (The build itself fails if `ORIGIN` and the feature disagree.)
#[test]
fn no_runtime_input_selects_the_origin() {
    use std::path::{Path, PathBuf};

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let module = std::fs::read_to_string(src.join("connectors.rs")).unwrap();
    for runtime_input in [
        "env::var",
        "var_os",
        "env::args",
        "args_os",
        "clauseo_settings",
        "Effective",
        "Policy",
        "LOCAL_ORIGIN = ",
        "policy::",
        "HostConfig::origin",
    ] {
        assert!(
            !module.contains(runtime_input),
            "connectors.rs reads {runtime_input}"
        );
    }
    assert_eq!(module.matches("config.origin = ").count(), 1);
    assert!(module.contains("config.origin = ORIGIN;"));
    assert!(module.contains("origin: ORIGIN,"));
    assert_eq!(module.matches("origin: ").count(), 1, "one relay origin");

    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let this_file = src.join("connectors").join("tests.rs");
    for file in files {
        if file == src.join("connectors.rs") || file == this_file {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for named in [
            "Origin::Staging",
            "Origin::Local",
            "Origin::Production",
            ".origin = ",
            "staging-connector",
            "local-connector",
            "8793",
        ] {
            assert!(
                !text.contains(named),
                "{} names {named}: the origin is chosen in connectors.rs only",
                file.display()
            );
        }
    }
}

/// The local test server's own links: in a local build its sign-in page and pay link open (the
/// pay link behind the confirm), and nothing else on loopback; in any other build none do.
#[test]
fn only_a_local_build_opens_the_local_server_s_links() {
    use std::sync::atomic::Ordering;

    let opener = Recorder::default();
    let page = "http://127.0.0.1:8793/authorize?state=s3cr3t";
    let pay_link = "http://127.0.0.1:8793/pay/rch_AB12cd34";
    let yes = Confirm {
        answer: true,
        asked: 0.into(),
    };
    for other in [
        "http://127.0.0.1:8794/authorize",
        "http://127.0.0.1/authorize",
        "https://127.0.0.1:8793/authorize",
        "http://localhost:8793/authorize",
        "http://user@127.0.0.1:8793/authorize",
        "http://127.0.0.1:8793",
        "http://127.0.0.1:87930/authorize",
    ] {
        assert!(open_link(&opener, other).is_err(), "{other}");
        assert!(pay(&yes, &opener, other).is_err(), "{other}");
    }
    assert_eq!(yes.asked.load(Ordering::SeqCst), 0);
    assert!(opener.0.lock().unwrap().is_empty());
    let opened = open_link(&opener, page).is_ok();
    let paid = pay(&yes, &opener, pay_link).is_ok();
    assert_eq!(opened, cfg!(feature = "local-connector"));
    assert_eq!(paid, cfg!(feature = "local-connector"));
    if cfg!(feature = "local-connector") {
        assert_eq!(*opener.0.lock().unwrap(), [page, pay_link]);
        assert_eq!(yes.asked.load(Ordering::SeqCst), 1);
    }
}
