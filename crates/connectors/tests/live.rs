//! Live tests against a real connector, ignored unless asked for, and skipped unless
//! `CONNECTORS_LIVE_ORIGIN` names one: `staging`, or `fake:http://127.0.0.1:<port>` for the
//! connector's pre-staging server on this machine. Run them with
//! `cargo nextest run -p clauseo-connectors --run-ignored only -E 'test(live_)'` (staging adds
//! `--features staging`).
//!
//! They print nothing secret: an error prints its kind or status only. The API key is read from
//! the file named by `CONNECTORS_LIVE_KEY_FILE` and never printed. No paid tool is called.

#[cfg(test)]
mod live {
    use std::path::PathBuf;
    use std::sync::Arc;

    use clauseo_connectors::fake::ScratchDir;
    use clauseo_connectors::{
        Config, ConnectorHandle, Connectors, MemoryStore, Origin, ProxyChoice, SignOutReport,
    };

    /// The connector named by `CONNECTORS_LIVE_ORIGIN`, or `None` (the test then skips).
    fn origin() -> Option<Origin> {
        let value = std::env::var("CONNECTORS_LIVE_ORIGIN").ok()?;
        #[cfg(feature = "staging")]
        if value == "staging" {
            return Some(Origin::Staging);
        }
        #[cfg(not(feature = "staging"))]
        assert!(
            value != "staging",
            "CONNECTORS_LIVE_ORIGIN=staging needs --features staging"
        );
        let url = value
            .strip_prefix("fake:")
            .and_then(|url| url::Url::parse(url).ok());
        assert!(
            url.is_some(),
            "CONNECTORS_LIVE_ORIGIN is `staging` or `fake:<url>`"
        );
        url.map(Origin::Fake)
    }

    /// Connectors on the live origin with a memory store; the scratch folder holds the state.
    fn connect(origin: Origin) -> (Connectors, ConnectorHandle, ScratchDir) {
        let scratch = ScratchDir::new("live").unwrap();
        let mut config =
            Config::production("Clauseo/0.0.0-live-test".to_owned(), scratch.state_file());
        if matches!(origin, Origin::Fake(_)) {
            config.proxy = ProxyChoice::Direct;
        }
        config.origin = origin;
        let connectors = Connectors::new(config, Arc::new(MemoryStore::new()))
            .unwrap_or_else(|err| panic!("{err}"));
        let handle = connectors.get("case-law").unwrap();
        (connectors, handle, scratch)
    }

    /// The first `cdk_live_` value in the key file, whatever its variable's name.
    fn key_from_file() -> String {
        let path = std::env::var("CONNECTORS_LIVE_KEY_FILE")
            .expect("CONNECTORS_LIVE_KEY_FILE names the env file holding a cdk_live_ key");
        let text = std::fs::read_to_string(PathBuf::from(path))
            .unwrap_or_else(|err| panic!("the key file could not be read ({:?})", err.kind()));
        let key = text.lines().find_map(|line| {
            let start = line.find("cdk_live_")?;
            let key: String = line[start..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            Some(key)
        });
        key.expect("the key file holds no cdk_live_ value")
    }

    #[test]
    #[ignore = "live: needs CONNECTORS_LIVE_ORIGIN"]
    fn live_discovery_and_pricing() {
        let Some(origin) = origin() else {
            eprintln!("SKIPPED: CONNECTORS_LIVE_ORIGIN is not set");
            return;
        };
        let (connectors, handle, _scratch) = connect(origin);
        let pending = handle
            .begin_sign_in(None)
            .unwrap_or_else(|err| panic!("discovery: {err}"));
        drop(pending);
        let liveness = connectors.liveness();
        let pricing = liveness.pricing.expect("pricing.json fetched and parsed");
        eprintln!(
            "live: {}, status {:?}, {} packs, {} prices",
            liveness.live,
            pricing.status,
            pricing.packs.len(),
            pricing.prices.len()
        );
    }

    #[test]
    #[ignore = "live: needs CONNECTORS_LIVE_ORIGIN and CONNECTORS_LIVE_KEY_FILE"]
    fn live_api_key_account() {
        let Some(origin) = origin() else {
            eprintln!("SKIPPED: CONNECTORS_LIVE_ORIGIN is not set");
            return;
        };
        let (_connectors, handle, _scratch) = connect(origin);
        let key = key_from_file();
        if let Err(err) = handle.add_api_key(&key) {
            panic!("add_api_key: {err}");
        }
        drop(key);
        let account = handle
            .account()
            .unwrap_or_else(|err| panic!("account: {err}"));
        assert_ne!(account.balance_inr.len(), 0, "the balance parses");
        eprintln!(
            "account: state {}, balance {} credits ({} INR)",
            account.state, account.balance_credits, account.balance_inr
        );
    }

    #[test]
    #[ignore = "live: needs CONNECTORS_LIVE_ORIGIN, CONNECTORS_SIGNIN_URL_FILE and a browser"]
    fn live_oauth_signin() {
        let Some(origin) = origin() else {
            eprintln!("SKIPPED: CONNECTORS_LIVE_ORIGIN is not set");
            return;
        };
        let (_connectors, handle, _scratch) = connect(origin);
        let pending = handle
            .begin_sign_in(None)
            .unwrap_or_else(|err| panic!("discovery: {err}"));
        write_url(pending.authorize_url().as_str());
        if let Err(err) = pending.finish() {
            panic!("sign-in: {err}");
        }
        let account = handle
            .account()
            .unwrap_or_else(|err| panic!("account: {err}"));
        assert_ne!(account.balance_inr.len(), 0, "the balance parses");
        let report = handle
            .sign_out()
            .unwrap_or_else(|err| panic!("sign_out: {err}"));
        assert_eq!(report, SignOutReport { revoked: true });
    }

    /// Writes the authorize URL (no secret in it) to `CONNECTORS_SIGNIN_URL_FILE`, for a person
    /// or a browser driver to open.
    #[expect(
        clippy::disallowed_methods,
        reason = "a live test hands the authorize URL to whoever opens the browser"
    )]
    fn write_url(url: &str) {
        let path = std::env::var("CONNECTORS_SIGNIN_URL_FILE")
            .expect("CONNECTORS_SIGNIN_URL_FILE names where the authorize URL goes");
        std::fs::write(path, url).unwrap();
        eprintln!("the authorize URL is in CONNECTORS_SIGNIN_URL_FILE; open it in a browser");
    }
}
