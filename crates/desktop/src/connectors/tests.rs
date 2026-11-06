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
