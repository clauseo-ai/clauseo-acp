//! Firm IT mode: IT's runtime and an offline bundle contact nothing; a mirror serves every
//! tarball.

use clauseo_agents::InstallState;
use clauseo_agents::testing::{TempDir, sha512_integrity};

use crate::support::{B_PACKAGES, Fixture, agent_a, agent_b, to_hex};
use crate::write;

#[test]
fn a_managed_id_and_an_offline_bundle_make_no_network_call() {
    let mut fx = Fixture::new("firm");
    fx.config.managed.insert(agent_a());
    let bundle = TempDir::new("bundle").unwrap();
    for name in B_PACKAGES {
        let bytes = &fx.tarballs[name];
        let integrity = sha512_integrity(bytes);
        let digest = base64_digest_hex(&integrity);
        write::write(
            &bundle.path().join("npm").join(format!("{digest}.tgz")),
            bytes,
        )
        .unwrap();
    }
    fx.config.install.bundle = Some(bundle.path().to_owned());
    let (agents, events) = fx.start();
    let (a, b) = events.first_states();
    assert_eq!(a, InstallState::Managed);
    assert!(matches!(b, InstallState::Missing { .. }), "{b:?}");
    agents.install(&agent_a());
    agents.install(&agent_b());
    let (seen, last) = events.until_end(&agent_b());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(seen.first(), Some(&(agent_a(), InstallState::Managed)));
    assert_eq!(agents.installed(&agent_a()), None);
    assert_eq!(fx.lookups.names(), Vec::<String>::new());
    assert_eq!(fx.registry.requests(), []);
    // The bundle's files stay.
    assert_eq!(
        std::fs::read_dir(bundle.path().join("npm"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn with_a_mirror_every_tarball_comes_from_the_mirror() {
    let mut fx = Fixture::new("mirror");
    fx.config.install.registry = Some("http://mirror.test/".to_owned());
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    let requests = fx.registry.requests();
    assert_eq!(requests.len(), 3);
    for request in requests {
        assert_eq!(request.host.as_deref(), Some("mirror.test"), "{request:?}");
    }
    let names = fx.lookups.names();
    assert_ne!(names, Vec::<String>::new());
    assert!(names.iter().all(|name| name == "mirror.test"), "{names:?}");
}

/// The lowercase hex of the digest in a `sha512-<base64>` integrity: a bundle's file name.
fn base64_digest_hex(integrity: &str) -> String {
    use base64::Engine as _;
    let b64 = integrity.strip_prefix("sha512-").unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    to_hex(&bytes)
}
