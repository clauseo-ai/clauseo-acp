//! No secret leaks: a whole run (sign-in, refresh, `mcp_post`, sign-out, the API key and the
//! error paths) under a capturing global `tracing` subscriber, with `LogTracer` turning ureq's
//! `log` records into events. Neither the captured output nor the `Debug` and `Display` text of
//! any public value holds a token, code, verifier or key the fake issued.
//!
//! Every error the run produces is checked by its `Debug` and its `Display` text.
//!
//! The fake is plain HTTP, so rustls never runs here and none of its `log` records can be
//! captured; the TLS path's logging is covered only by rustls' own log levels (it logs no key
//! material) and by ureq setting no key log file.
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
        McpRequestHeaders, MemoryStore, PayUrl, PendingSignIn, ProxyChoice, SignInError,
        StoreError, classify_tool_result,
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

    /// A result whose `Debug` text also holds the error's `Display` text.
    struct Shown<'a, T, E>(&'a Result<T, E>);

    impl<T: std::fmt::Debug, E: std::fmt::Debug + std::fmt::Display> std::fmt::Debug
        for Shown<'_, T, E>
    {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.0 {
                Ok(value) => write!(f, "Ok({value:?})"),
                Err(err) => write!(f, "Err({err:?}) shown as {err}"),
            }
        }
    }

    /// The proxy password the `Debug` text of a `Config` and of `Connectors` must never show.
    const PROXY_PASSWORD: &str = "pw-secret";

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

        // Discovery refused, the PKCE mismatch and the token endpoint failing; then the real
        // sign-in. The stray wrong-state callback comes at the end, by hand.
        sign_in_failures(&fake, &handle, &mut debug);

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
        debug(&Shown(
            &handle.wait_recharge(&recharge.recharge_id, &CancelFlag::new()),
        ));

        // Refresh by expiry, the 401 retry, a refresh refused, and every error after it.
        clock.advance(Duration::from_secs(3600));
        debug(&handle.bearer().unwrap());
        fake.expire_access_tokens();
        debug(&Shown(&handle.mcp_post(call.as_bytes(), &headers)));
        fake.fail_next("/token", 500);
        clock.advance(Duration::from_secs(3600));
        debug(&Shown(&handle.bearer()));
        fake.close_next("/token");
        debug(&Shown(&handle.bearer()));
        store.fail_next_writes(2);
        debug(&Shown(&handle.bearer()));
        debug(&handle.persist_failed());
        debug(&handle);
        debug(&connectors);
        debug(&store);
        fake.revoke_all();
        clock.advance(Duration::from_secs(3600));
        debug(&Shown(&handle.bearer()));
        debug(&Shown(&handle.account()));

        // Sign in again and sign out, then the API key: refused, accepted, revoked.
        let pending = handle.begin_sign_in(None).unwrap();
        complete(&fake, pending).1.unwrap();
        debug(&Shown(&handle.sign_out()));
        debug(&Shown(
            &handle.add_api_key("cdk_live_0123456789abcdefghijABCDEFGHIJ01"),
        ));
        debug(&Shown(&handle.add_api_key("not a key")));
        let key = fake.new_api_key();
        handle.add_api_key(&key).unwrap();
        debug(&handle.bearer().unwrap());
        fake.revoke_all();
        clock.advance(Duration::from_secs(120));
        debug(&Shown(&handle.account()));
        debug(&Shown(&handle.remove()));

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
        debug(&Shown(&finisher.join().unwrap()));

        proxied(&mut debug, &config);
        other_types(&mut debug, &config, &clock);
        drop(debug);
        let errors = texts;

        assert_no_secret(&fake, key, &captured, &errors);
    }
    /// Discovery refused (an issuer one character off, then a 500), before any metadata is kept
    /// for the process; then the PKCE mismatch and the token endpoint failing (a 500, then a
    /// connection closed with no answer).
    fn sign_in_failures(
        fake: &clauseo_connectors::fake::FakeHandle,
        handle: &clauseo_connectors::ConnectorHandle,
        debug: &mut dyn FnMut(&dyn std::fmt::Debug),
    ) {
        let good_metadata = fake.as_metadata();
        let mut bad_metadata = good_metadata.clone();
        bad_metadata["issuer"] = json!(format!("{}x", fake.base()));
        fake.set_as_metadata(bad_metadata);
        debug(&Shown(&handle.begin_sign_in(None)));
        fake.set_as_metadata(good_metadata);
        fake.fail_next("/.well-known/oauth-authorization-server", 500);
        debug(&Shown(&handle.begin_sign_in(None)));

        let mut tampered = handle.begin_sign_in(Some("advocate@example.in")).unwrap();
        tampered.tamper_verifier_for_test();
        let (_, mismatch) = complete(fake, tampered);
        debug(&Shown(&mismatch));
        fake.fail_next("/token", 500);
        let (_, refused) = complete(fake, handle.begin_sign_in(None).unwrap());
        debug(&Shown(&refused));
        fake.close_next("/token");
        let (_, closed) = complete(fake, handle.begin_sign_in(None).unwrap());
        assert!(
            matches!(closed, Err(SignInError::Exchange(ref why)) if why.starts_with("network")),
            "{closed:?}"
        );
        debug(&Shown(&closed));
    }

    /// A proxy with a password: the `Debug` text of the config and of `Connectors`.
    fn proxied(debug: &mut dyn FnMut(&dyn std::fmt::Debug), config: &clauseo_connectors::Config) {
        let mut proxied = config.clone();
        proxied.proxy = ProxyChoice::Fixed(
            url::Url::parse(&format!("http://user:{PROXY_PASSWORD}@127.0.0.1:9")).unwrap(),
        );
        debug(&proxied);
        debug(&proxied.proxy);
        let proxied_connectors = Connectors::new(proxied, Arc::new(MemoryStore::new())).unwrap();
        debug(&proxied_connectors);
        assert!(
            format!("{proxied_connectors:?}").contains("Fixed(http://127.0.0.1:9)"),
            "{proxied_connectors:?}"
        );
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
        debug(&Shown::<(), _>(&Err(Error::Auth(AuthError::Network(
            "x".to_owned(),
        )))));
        debug(&Shown::<(), _>(&Err(StoreError::Unsupported)));
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
        secrets.push(PROXY_PASSWORD.to_owned());
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
