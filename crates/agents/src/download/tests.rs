use std::path::{Path, PathBuf};
use std::sync::mpsc;

use clauseo_protocol::AgentId;

use super::firm::{BundleEntry, bundle_file, mirror_url};
use super::*;
use crate::snapshot::NPM_REGISTRY;
use crate::testing::{
    Behaviour, TempDir, TestRegistry, npm_tarball, offline_resolver, resolve_all_to,
    sha512_integrity, write_file,
};

const PATH: &str = "/pkg/-/pkg-1.0.0.tgz";
const RESOLVED: &str = "https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz";

fn agent() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

/// A gzip'd tarball of about 150 KB, so a download takes several 64 KiB chunks.
fn fixture() -> Vec<u8> {
    let mut noise = Vec::with_capacity(150_000);
    let mut x: u32 = 0x1234_5678;
    for _ in 0..150_000 {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        noise.push((x >> 24) as u8);
    }
    npm_tarball("pkg", "1.0.0", &[("noise.bin", &noise)]).unwrap()
}

fn expected(bytes: &[u8]) -> Expected {
    Expected::from_integrity(&sha512_integrity(bytes)).unwrap()
}

fn job(url: &str, bytes: &[u8], dest: PathBuf) -> FetchJob {
    FetchJob {
        url: url.to_owned(),
        expected: expected(bytes),
        dest,
        magic: Magic::Gzip,
    }
}

fn fetcher(install: &InstallConfig, data: &Path) -> Fetcher {
    Fetcher::new(install, &agent(), None, data).unwrap()
}

fn fetch(fetcher: &Fetcher, job: &FetchJob) -> Result<u64, FetchError> {
    fetcher.fetch(job, &Cancel::new(), &mut |_, _| {})
}

/// A registry serving `fixture()` at [`PATH`], a temp data folder, and a default fetcher.
fn setup(label: &str) -> (TestRegistry, Vec<u8>, TempDir, Fetcher) {
    let registry = TestRegistry::start().unwrap();
    let bytes = fixture();
    registry.route(PATH, bytes.clone());
    let dir = TempDir::new(label).unwrap();
    let fetcher = fetcher(&InstallConfig::default(), dir.path());
    (registry, bytes, dir, fetcher)
}

fn len_of(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap().len()
}

#[test]
fn a_cut_download_resumes_by_range_to_the_full_verified_file() {
    let (registry, bytes, dir, fetcher) = setup("cut");
    registry.then(PATH, Behaviour::CutAfter(70_000));
    let job = job(&registry.url(PATH), &bytes, dir.path().join("pkg.tgz"));

    let err = fetch(&fetcher, &job).unwrap_err();
    assert_eq!(err.kind, FetchKind::Failed, "{err:?}");
    let part = part_path(&job.dest);
    assert_eq!(len_of(&part), 70_000);

    let mut seen = Vec::new();
    let size = fetcher
        .fetch(&job, &Cancel::new(), &mut |received, total| {
            seen.push((received, total));
        })
        .unwrap();
    let full = bytes.len() as u64;
    assert_eq!(size, full);
    assert_eq!(std::fs::read(&job.dest).unwrap(), bytes);
    assert!(!part.exists());
    let requests = registry.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].range, None);
    assert_eq!(requests[1].range.as_deref(), Some("bytes=70000-"));
    assert!(seen.iter().all(|&(_, total)| total == full), "{seen:?}");
    assert!(seen.first().unwrap().0 > 70_000);
    assert_eq!(seen.last().unwrap().0, full);
}

#[test]
fn a_tampered_file_is_an_integrity_failure_and_leaves_no_file() {
    let (registry, bytes, dir, fetcher) = setup("tamper");
    registry.then(PATH, Behaviour::Tamper);
    let job = job(&registry.url(PATH), &bytes, dir.path().join("pkg.tgz"));
    let err = fetch(&fetcher, &job).unwrap_err();
    assert_eq!(err.kind, FetchKind::Integrity);
    assert!(!job.dest.exists());
    assert!(!part_path(&job.dest).exists());
}

#[test]
fn a_403_or_407_is_blocked_with_its_status_and_a_404_failed() {
    let (registry, bytes, dir, fetcher) = setup("status");
    let job = job(&registry.url(PATH), &bytes, dir.path().join("pkg.tgz"));
    for (code, kind) in [
        (403, FetchKind::Blocked),
        (407, FetchKind::Blocked),
        (404, FetchKind::Failed),
        (500, FetchKind::Failed),
    ] {
        registry.then(PATH, Behaviour::Status(code));
        let err = fetch(&fetcher, &job).unwrap_err();
        assert_eq!((err.kind, err.status), (kind, Some(code)), "{err:?}");
        assert_eq!(err.host, Some(registry.addr().to_string()));
    }
    let err = fetch(
        &fetcher,
        &job_at(&registry.url("/missing.tgz"), &bytes, &dir),
    )
    .unwrap_err();
    assert_eq!((err.kind, err.status), (FetchKind::Failed, Some(404)));
}

fn job_at(url: &str, bytes: &[u8], dir: &TempDir) -> FetchJob {
    job(url, bytes, dir.path().join("other.tgz"))
}

#[test]
fn https_to_a_plain_http_server_is_a_tls_failure_so_blocked() {
    let (registry, bytes, dir, fetcher) = setup("tls");
    let url = format!("https://{}{PATH}", registry.addr());
    let err = fetch(&fetcher, &job_at(&url, &bytes, &dir)).unwrap_err();
    assert_eq!(err.kind, FetchKind::Blocked, "{err:?}");
    assert_eq!(err.status, None);
}

#[test]
fn a_web_page_is_blocked_and_a_file_of_the_wrong_kind_failed() {
    let (registry, bytes, dir, fetcher) = setup("html");
    registry.then(PATH, Behaviour::Html);
    let job = job(&registry.url(PATH), &bytes, dir.path().join("pkg.tgz"));
    let err = fetch(&fetcher, &job).unwrap_err();
    assert_eq!(err.kind, FetchKind::Blocked, "{err:?}");

    // No HTML content type, but the body starts with `<`.
    let page = b"\n<html>sign in to continue</html>".to_vec();
    registry.route("/page.tgz", page.clone());
    let err = fetch(&fetcher, &job_at(&registry.url("/page.tgz"), &page, &dir)).unwrap_err();
    assert_eq!(err.kind, FetchKind::Blocked, "{err:?}");
    assert!(!dir.path().join("other.tgz.part").exists());

    let zip = b"PK\x03\x04 not gzip".to_vec();
    registry.route("/zip.tgz", zip.clone());
    let err = fetch(&fetcher, &job_at(&registry.url("/zip.tgz"), &zip, &dir)).unwrap_err();
    assert_eq!(err.kind, FetchKind::Failed, "{err:?}");
    assert!(err.detail.contains("gzip"), "{}", err.detail);
}

#[test]
fn a_failed_lookup_is_offline_and_a_refused_port_failed() {
    let dir = TempDir::new("offline").unwrap();
    let bytes = fixture();
    let install = InstallConfig {
        resolver: Some(offline_resolver()),
        ..InstallConfig::default()
    };
    let offline = fetcher(&install, dir.path());
    let url = "http://registry.test/pkg.tgz";
    let err = fetch(&offline, &job_at(url, &bytes, &dir)).unwrap_err();
    assert_eq!(err.kind, FetchKind::Offline, "{err:?}");
    assert_eq!(err.host.as_deref(), Some("registry.test"));

    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let plain = fetcher(&InstallConfig::default(), dir.path());
    let url = format!("http://127.0.0.1:{port}/pkg.tgz");
    let err = fetch(&plain, &job_at(&url, &bytes, &dir)).unwrap_err();
    assert_eq!(err.kind, FetchKind::Failed, "{err:?}");
}

#[test]
fn a_mirror_s_user_name_and_password_never_reach_the_error() {
    let (registry, bytes, dir, _) = setup("mirror-secret");
    registry.then(PATH, Behaviour::Status(403));
    let (resolver, _) = resolve_all_to(registry.addr());
    let install = InstallConfig {
        resolver: Some(resolver),
        ..InstallConfig::default()
    };
    let fetcher = fetcher(&install, dir.path());
    let url = mirror_url(
        RESOLVED,
        NPM_REGISTRY,
        "http://rohan:hunter2@mirror.test/",
        dir.path(),
    )
    .unwrap();
    let err = fetch(&fetcher, &job_at(&url, &bytes, &dir)).unwrap_err();
    assert_eq!((err.kind, err.status), (FetchKind::Blocked, Some(403)));
    assert_eq!(err.host.as_deref(), Some("mirror.test"));
    for secret in ["rohan", "hunter2"] {
        assert!(!err.detail.contains(secret), "{}", err.detail);
        assert!(!err.to_string().contains(secret));
    }
}

#[test]
fn with_a_mirror_every_request_goes_to_the_mirror() {
    let (registry, bytes, dir, _) = setup("mirror");
    let other = npm_tarball("other", "2.0.0", &[]).unwrap();
    registry.route("/other/-/other-2.0.0.tgz", other.clone());
    let (resolver, lookups) = resolve_all_to(registry.addr());
    let install = InstallConfig {
        resolver: Some(resolver),
        ..InstallConfig::default()
    };
    let fetcher = fetcher(&install, dir.path());
    let resolved = [
        (RESOLVED, &bytes, "pkg.tgz"),
        (
            "https://registry.npmjs.org/other/-/other-2.0.0.tgz",
            &other,
            "other.tgz",
        ),
    ];
    for (url, bytes, name) in resolved {
        let url = mirror_url(url, NPM_REGISTRY, "http://mirror.test", dir.path()).unwrap();
        fetch(&fetcher, &job(&url, bytes, dir.path().join(name))).unwrap();
    }
    assert_eq!(lookups.names(), ["mirror.test", "mirror.test"]);
    let hosts: Vec<_> = registry.requests().into_iter().map(|r| r.host).collect();
    assert_eq!(
        hosts,
        [
            Some("mirror.test".to_owned()),
            Some("mirror.test".to_owned())
        ]
    );
}

#[test]
fn the_mirror_base_replaces_the_registry_s_and_other_urls_are_refused() {
    let data = Path::new("/data");
    let want = "https://npm.firm.example/repo/pkg/-/pkg-1.0.0.tgz";
    for base in [
        "https://npm.firm.example/repo",
        "https://npm.firm.example/repo/",
    ] {
        assert_eq!(
            mirror_url(RESOLVED, NPM_REGISTRY, base, data).unwrap(),
            want
        );
    }
    let err = mirror_url(
        "https://evil.example/pkg.tgz",
        NPM_REGISTRY,
        "https://npm.firm.example",
        data,
    );
    assert_eq!(err.unwrap_err().kind, FetchKind::Failed);
    let err = mirror_url(RESOLVED, NPM_REGISTRY, "npm.firm.example", data).unwrap_err();
    assert_eq!(err.kind, FetchKind::Failed);
}

#[test]
fn a_bundle_hit_makes_no_request_and_a_miss_or_mismatch_fails() {
    let registry = TestRegistry::start().unwrap();
    let dir = TempDir::new("bundle").unwrap();
    let bundle = dir.path().join("bundle");
    let bytes = fixture();
    let want = expected(&bytes);
    let at = bundle.join("npm").join(format!("{}.tgz", want.hex()));
    write_file(&at, &bytes).unwrap();
    let found = bundle_file(
        &bundle,
        BundleEntry::Npm,
        &want,
        "pkg-1.0.0.tgz",
        dir.path(),
    );
    assert_eq!(found.unwrap(), at);
    assert_eq!(registry.requests(), Vec::new());

    let missing = expected(b"another package");
    let err = bundle_file(
        &bundle,
        BundleEntry::Npm,
        &missing,
        "other-2.0.0.tgz",
        dir.path(),
    )
    .unwrap_err();
    assert_eq!(err.kind, FetchKind::Failed);
    assert!(err.detail.contains("other-2.0.0.tgz"), "{}", err.detail);
    assert!(!err.detail.contains(&*dir.path().to_string_lossy()));

    let node = "node-v24.22.0-linux-x64.tar.gz";
    write_file(&bundle.join("node").join(node), b"not the archive").unwrap();
    let err = bundle_file(&bundle, BundleEntry::Node(node), &want, node, dir.path()).unwrap_err();
    assert_eq!(err.kind, FetchKind::Failed);
    assert!(err.detail.contains("checksum"), "{}", err.detail);
    let err = bundle_file(&bundle, BundleEntry::Node("../x"), &want, "x", dir.path());
    assert_eq!(err.unwrap_err().kind, FetchKind::Failed);
}

#[test]
fn a_complete_destination_makes_no_request_and_a_wrong_one_is_replaced() {
    let (registry, bytes, dir, fetcher) = setup("complete");
    let job = job(&registry.url(PATH), &bytes, dir.path().join("pkg.tgz"));
    write_file(&job.dest, &bytes).unwrap();
    let mut seen = Vec::new();
    let size = fetcher
        .fetch(&job, &Cancel::new(), &mut |r, t| seen.push((r, t)))
        .unwrap();
    assert_eq!(size, bytes.len() as u64);
    assert_eq!(seen, [(size, size)]);
    assert_eq!(registry.requests(), Vec::new());

    write_file(&job.dest, b"stale").unwrap();
    fetch(&fetcher, &job).unwrap();
    assert_eq!(std::fs::read(&job.dest).unwrap(), bytes);
    assert_eq!(registry.requests().len(), 1);
}

#[test]
fn a_pause_during_a_stall_keeps_the_part_and_the_next_fetch_resumes_it() {
    let (registry, bytes, dir, fetcher) = setup("stall");
    registry.then(PATH, Behaviour::Stall);
    let job = job(&registry.url(PATH), &bytes, dir.path().join("pkg.tgz"));
    let cancel = Cancel::new();
    let (got, progress) = mpsc::channel();
    let result = std::thread::scope(|scope| {
        let running = scope.spawn(|| {
            fetcher.fetch(&job, &cancel, &mut |received, _| {
                got.send(received).unwrap_or_default();
            })
        });
        progress.recv().unwrap();
        registry.wait_for_stalls(1);
        cancel.cancel();
        registry.release();
        running.join().unwrap()
    });
    assert_eq!(result.unwrap_err().kind, FetchKind::Cancelled);
    let part = part_path(&job.dest);
    let kept = len_of(&part);
    assert!(kept > 0 && kept < bytes.len() as u64, "{kept}");
    assert!(!job.dest.exists());

    fetch(&fetcher, &job).unwrap();
    assert_eq!(std::fs::read(&job.dest).unwrap(), bytes);
    let last = registry.requests().pop().unwrap();
    assert_eq!(last.range, Some(format!("bytes={kept}-")));
}

#[test]
fn a_416_deletes_the_part_and_starts_over_once() {
    let (registry, bytes, dir, fetcher) = setup("416");
    let job = job(&registry.url(PATH), &bytes, dir.path().join("pkg.tgz"));
    let mut too_long = bytes.clone();
    too_long.extend_from_slice(b"extra");
    write_file(&part_path(&job.dest), &too_long).unwrap();
    fetch(&fetcher, &job).unwrap();
    assert_eq!(std::fs::read(&job.dest).unwrap(), bytes);
    let ranges: Vec<_> = registry.requests().into_iter().map(|r| r.range).collect();
    assert_eq!(ranges, [Some(format!("bytes={}-", too_long.len())), None]);
}

#[test]
fn through_a_tls_inspecting_proxy_the_ca_file_is_trusted_and_without_it_blocked() {
    let registry = TestRegistry::start_tls_proxy().unwrap();
    let bytes = fixture();
    registry.route(PATH, bytes.clone());
    let dir = TempDir::new("proxy").unwrap();
    let ca = dir.path().join("ca.pem");
    write_file(&ca, registry.ca_pem().unwrap().as_bytes()).unwrap();
    let url = format!("https://registry.test{PATH}");

    let mut install = InstallConfig {
        proxy: Some(registry.proxy_url()),
        ..InstallConfig::default()
    };
    let untrusting = fetcher(&install, dir.path());
    let err = fetch(&untrusting, &job_at(&url, &bytes, &dir)).unwrap_err();
    assert_eq!(err.kind, FetchKind::Blocked, "{err:?}");

    install.ca_certs.insert(agent(), ca);
    let trusting = fetcher(&install, dir.path());
    let job = job(&url, &bytes, dir.path().join("pkg.tgz"));
    assert_eq!(fetch(&trusting, &job).unwrap(), bytes.len() as u64);
    assert_eq!(std::fs::read(&job.dest).unwrap(), bytes);
    let last = registry.requests().pop().unwrap();
    assert!(last.tunnelled);
    assert_eq!(last.host.as_deref(), Some("registry.test"));
    assert!(
        registry
            .requests()
            .iter()
            .any(|r| r.method == "CONNECT" && r.path == "registry.test:443")
    );
}

#[test]
fn a_ca_file_with_no_certificate_or_a_bad_proxy_fails_to_build() {
    let dir = TempDir::new("build").unwrap();
    let ca = dir.path().join("empty.pem");
    write_file(&ca, b"no certificates here\n").unwrap();
    let install = InstallConfig {
        ca_certs: [(agent(), ca)].into(),
        ..InstallConfig::default()
    };
    let err = Fetcher::new(&install, &agent(), None, dir.path()).unwrap_err();
    assert_eq!(err.kind, FetchKind::Failed);
    // The setting is named, never its value: no path, not even the data folder's.
    assert_eq!(
        err.detail,
        "the CA file for claude-acp holds no certificate"
    );

    let install = InstallConfig {
        proxy: Some("http://user:pw@:not a proxy".to_owned()),
        ..InstallConfig::default()
    };
    let err = Fetcher::new(&install, &agent(), None, dir.path()).unwrap_err();
    assert_eq!(
        err.detail,
        "the proxy setting is not usable: invalid proxy url"
    );
}

#[test]
fn the_head_check_finds_pages_and_wrong_kinds() {
    assert_eq!(check_head(b"\x1f\x8b\x08", Magic::Gzip), Ok(()));
    assert_eq!(check_head(b"PK\x03\x04", Magic::Zip), Ok(()));
    assert_eq!(
        check_head(b"  <!DOCTYPE html>", Magic::Gzip),
        Err(FetchKind::Blocked)
    );
    assert_eq!(check_head(b"PK", Magic::Gzip), Err(FetchKind::Failed));
    assert_eq!(check_head(b"", Magic::Gzip), Err(FetchKind::Failed));
    assert!(is_html_type("text/html; charset=utf-8"));
    assert!(is_html_type("TEXT/HTML"));
    assert!(!is_html_type("application/octet-stream"));
}
