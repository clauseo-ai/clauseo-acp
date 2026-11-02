//! No secret leaks: a whole run (sign-in, refresh, `mcp_post`, sign-out, the API key and the
//! error paths) under a capturing global `tracing` subscriber, with `LogTracer` turning ureq's
//! `log` records into events. Neither the captured output nor the `Debug` and `Display` text of
//! any public value holds a token, code, verifier or key the fake issued.
//!
//! Its own test binary, because the subscriber and `LogTracer` are process-wide.

#[cfg(test)]
mod leak {
    use std::io;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::thread;
    use std::time::Duration;

    use clauseo_connectors::fake::{FakeConnector, FakeOptions, RawResponse, ScratchDir, http};
    use clauseo_connectors::{
        AuthError, CASE_LAW, CancelFlag, Connectors, Error, KeyringStore, ManualClock,
        McpRequestHeaders, MemoryStore, PayUrl, PendingSignIn, SignInError, StoreError,
        classify_tool_result,
    };
    use serde_json::json;
    use tracing_subscriber::fmt::MakeWriter;

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

    fn complete(
        fake: &clauseo_connectors::fake::FakeHandle,
        pending: PendingSignIn,
    ) -> (RawResponse, Result<(), SignInError>) {
        let url = pending.authorize_url().clone();
        let finisher = thread::spawn(move || pending.finish());
        let page = fake.drive_authorize(&url).unwrap();
        (page, finisher.join().unwrap())
    }

    #[test]
    fn a_whole_run_leaks_no_secret_to_logs_or_debug_text() {
        tracing_log::LogTracer::init().unwrap();
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(captured.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber).unwrap();

        let clock = Arc::new(ManualClock::default());
        let fake = FakeConnector::start(FakeOptions {
            clock: Arc::<ManualClock>::clone(&clock),
            ..FakeOptions::default()
        })
        .unwrap();
        let scratch = ScratchDir::new("leak").unwrap();
        let store = Arc::new(MemoryStore::new());
        let config = fake.config(scratch.state_file());
        let connectors = Connectors::new(
            config.clone(),
            Arc::clone(&store) as Arc<dyn clauseo_connectors::SecretStore>,
        )
        .unwrap();
        let handle = connectors.get("case-law").unwrap();
        let mut texts: Vec<String> = Vec::new();
        let mut debug = |value: &dyn std::fmt::Debug| texts.push(format!("{value:?} {value:#?}"));

        // Sign in, with a stray wrong-state callback first, and the PKCE mismatch.
        let mut tampered = handle.begin_sign_in(Some("advocate@example.in")).unwrap();
        tampered.tamper_verifier_for_test();
        let (_, mismatch) = complete(&fake, tampered);
        debug(&mismatch);
        let pending = handle.begin_sign_in(None).unwrap();
        debug(&pending);
        debug(&pending.cancel_flag());
        let (page, signed_in) = complete(&fake, pending);
        signed_in.unwrap();
        debug(&page.status);
        debug(&handle.status());

        // Liveness, the account, bearer, mcp_post (JSON and SSE), a recharge and its poll.
        debug(&connectors.liveness());
        debug(&handle.account().unwrap());
        debug(&handle.bearer().unwrap());
        let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "citator", "arguments": {}}})
        .to_string();
        let headers = McpRequestHeaders {
            protocol_version: Some("2025-06-18".to_owned()),
            session_id: None,
        };
        debug(&headers);
        debug(&handle.mcp_post(call.as_bytes(), &headers).unwrap());
        fake.respond_as_sse(true);
        debug(&handle.mcp_post(call.as_bytes(), &headers).unwrap());
        fake.respond_as_sse(false);
        let recharge = handle.create_recharge("200").unwrap();
        debug(&recharge);
        fake.mark_paid(&recharge.recharge_id);
        debug(&handle.wait_recharge(&recharge.recharge_id, &CancelFlag::new()));

        // Refresh by expiry, the 401 retry, a refresh refused, and every error after it.
        clock.advance(Duration::from_secs(3600));
        debug(&handle.bearer().unwrap());
        fake.expire_access_tokens();
        debug(&handle.mcp_post(call.as_bytes(), &headers));
        fake.fail_next("/token", 500);
        clock.advance(Duration::from_secs(3600));
        debug(&handle.bearer());
        store.fail_next_writes(2);
        debug(&handle.bearer());
        debug(&handle.persist_failed());
        debug(&handle);
        debug(&connectors);
        debug(&store);
        fake.revoke_all();
        clock.advance(Duration::from_secs(3600));
        debug(&handle.bearer());
        debug(&handle.account());

        // Sign in again and sign out, then the API key: refused, accepted, revoked.
        let pending = handle.begin_sign_in(None).unwrap();
        complete(&fake, pending).1.unwrap();
        debug(&handle.sign_out());
        debug(&handle.add_api_key("cdk_live_0123456789abcdefghijABCDEFGHIJ01"));
        debug(&handle.add_api_key("not a key"));
        let key = fake.new_api_key();
        handle.add_api_key(&key).unwrap();
        debug(&handle.bearer().unwrap());
        fake.revoke_all();
        clock.advance(Duration::from_secs(120));
        debug(&handle.account());
        debug(&handle.remove());

        // A callback with the wrong state, by hand.
        let pending = handle.begin_sign_in(None).unwrap();
        let redirect = pending
            .authorize_url()
            .query_pairs()
            .find(|(name, _)| name == "redirect_uri")
            .unwrap()
            .1
            .into_owned();
        let stray = url::Url::parse(&format!("{redirect}?code=fake_code_stray&state=x")).unwrap();
        let flag = pending.cancel_flag();
        let finisher = thread::spawn(move || pending.finish());
        debug(&http("GET", &stray, &[], &[]).unwrap().status);
        flag.cancel();
        debug(&finisher.join().unwrap());

        other_types(&mut debug, &config, &clock);
        drop(debug);
        let errors = texts;

        assert_no_secret(&fake, key, &captured, &errors);
    }
    /// The Debug text of the public types the run did not produce.
    fn other_types(
        debug: &mut dyn FnMut(&dyn std::fmt::Debug),
        config: &clauseo_connectors::Config,
        clock: &ManualClock,
    ) {
        debug(config);
        debug(&CASE_LAW);
        debug(&KeyringStore::new());
        debug(clock);
        debug(&PayUrl::parse(
            &CASE_LAW,
            "https://data.clauseo.chat/pay/r1",
        ));
        debug(&classify_tool_result(&CASE_LAW, &json!({"isError": true})));
        debug(&Error::Auth(AuthError::Network("x".to_owned())));
        debug(&StoreError::Unsupported);
    }

    /// Asserts no issued secret, verifier or key is in the log or any captured text.
    fn assert_no_secret(
        fake: &clauseo_connectors::fake::FakeHandle,
        key: String,
        captured: &Captured,
        errors: &[String],
    ) {
        let mut secrets = fake.issued_secrets();
        assert!(secrets.iter().any(|s| s.starts_with("fake_code_")));
        assert!(secrets.iter().any(|s| s.starts_with("fake_at_")));
        assert!(secrets.iter().any(|s| s.starts_with("fake_rt_")));
        secrets.push(key);
        for token in fake.requests_to("/token") {
            for (name, value) in token.form() {
                if name == "code_verifier" {
                    secrets.push(value);
                }
            }
        }
        assert!(
            secrets.iter().filter(|s| s.len() == 43).count() >= 3,
            "the verifiers"
        );
        let log = String::from_utf8_lossy(&captured.0.lock().unwrap()).into_owned();
        assert!(log.contains("ureq"), "ureq's records are captured");
        for secret in &secrets {
            assert!(!log.contains(secret.as_str()), "the log holds a secret");
            for text in errors {
                assert!(
                    !text.contains(secret.as_str()),
                    "a Debug text holds a secret: {text}"
                );
            }
        }
    }
}
