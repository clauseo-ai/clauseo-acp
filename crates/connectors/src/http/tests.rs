use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use tracing_subscriber::fmt::MakeWriter;

use super::*;
use crate::fake::{FakeConnector, FakeOptions, ScratchDir};

/// Everything the subscriber writes.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Captured {
        self.clone()
    }
}

#[test]
fn the_agent_reaches_the_fake_without_logging_the_bearer_token() {
    // ureq and rustls log through `log`; LogTracer turns those records into tracing events.
    if let Err(err) = tracing_log::LogTracer::init() {
        panic!("{err}");
    }
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(captured.clone())
        .finish();
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let key = fake.new_api_key();
    let scratch = ScratchDir::new("http").unwrap();
    let config = fake.config(scratch.state_file());
    let agent = agent(&config).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        let response = agent
            .get(fake.url("/v1/case-law/account").as_str())
            .header("Authorization", &format!("Bearer {key}"))
            .call()
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        let refused = agent
            .get(fake.url("/v1/case-law/account").as_str())
            .call()
            .unwrap();
        assert_eq!(refused.status().as_u16(), 401, "a status is not an error");
    });
    let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(
        text.contains("ureq"),
        "ureq's records were captured: {text}"
    );
    assert!(!text.contains(&key), "the key leaked into the log");
    let request = &fake.requests()[0];
    assert_eq!(request.header("user-agent"), Some("Clauseo/0.0.0-test"));
}

#[test]
fn production_refuses_plain_http_and_a_bad_fixed_proxy_is_an_error() {
    let mut config = Config::production("Clauseo/test".to_owned(), "state.json".into());
    config.proxy = ProxyChoice::Direct;
    let agent = agent(&config).unwrap();
    // Refused before any connection: the port is never contacted.
    assert!(agent.get("http://127.0.0.1:9/pricing.json").call().is_err());
    config.proxy = ProxyChoice::Fixed(url::Url::parse("ftp://user:pw@proxy.example").unwrap());
    let err = super::agent(&config).unwrap_err();
    assert!(!err.to_string().contains("pw"), "{err}");
}
