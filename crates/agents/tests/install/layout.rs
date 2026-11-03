//! The install folders: one `<hash>` per agent with only its own packages, an incomplete
//! folder redone, no links, and a Node below 22 on `PATH` passed over.

use std::collections::BTreeSet;

use clauseo_agents::{InstallState, Installed};

use crate::support::{
    A_PACKAGES, B_PACKAGES, Events, Fixture, agent_a, agent_b, links_under, tarball_path,
};
use crate::write;

/// Installs `agent` and gives its `Installed`.
fn install_ready(
    agents: &clauseo_agents::Agents,
    events: &Events,
    agent: &clauseo_protocol::AgentId,
) -> Installed {
    agents.install(agent);
    let (_, last) = events.until_end(agent);
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    agents.installed(agent).unwrap()
}

/// The tarball paths requested so far, from request `from` on.
fn paths_since(fx: &Fixture, from: usize) -> BTreeSet<String> {
    fx.registry.requests()[from..]
        .iter()
        .map(|request| request.path.clone())
        .collect()
}

#[test]
fn each_agent_fetches_only_its_own_packages_into_its_own_hash_folder() {
    let fx = Fixture::new("separate");
    let (agents, events) = fx.start();
    events.first_states();
    let a = install_ready(&agents, &events, &agent_a());
    let after_a = fx.registry.requests().len();
    let want_a: BTreeSet<String> = A_PACKAGES.iter().map(|n| tarball_path(n)).collect();
    assert_eq!(paths_since(&fx, 0), want_a);
    let b = install_ready(&agents, &events, &agent_b());
    let want_b: BTreeSet<String> = B_PACKAGES.iter().map(|n| tarball_path(n)).collect();
    assert_eq!(paths_since(&fx, after_a), want_b);
    assert_ne!(a.folder.file_name(), b.folder.file_name());
    for (installed, root, other) in [
        (&a, "adapter-a", "adapter-b"),
        (&b, "adapter-b", "adapter-a"),
    ] {
        assert!(installed.folder.join("clauseo-receipt.json").is_file());
        assert!(installed.script.is_file(), "{}", installed.script.display());
        assert!(installed.folder.join("node_modules").join(root).is_dir());
        assert!(!installed.folder.join("node_modules").join(other).exists());
    }
    assert!(a.folder.join("node_modules/peer-a").is_dir());
    assert!(a.folder.join("node_modules/dep-a").is_dir());
    assert!(b.folder.join("node_modules/dep-b").is_dir());
}

#[test]
fn a_folder_with_no_receipt_is_redone() {
    let fx = Fixture::new("no-receipt");
    let hash = fx
        .config
        .snapshot
        .install_hash(&agent_a(), fx.config.platform)
        .unwrap();
    let folder = fx.install_root(&agent_a()).join(&hash);
    write::write(&folder.join("node_modules/adapter-a/stale.js"), "stale").unwrap();
    let tmp = fx.install_root(&agent_a()).join(format!(".tmp-{hash}-1"));
    write::write(&tmp.join("package.json"), "{}").unwrap();
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Missing { .. }), "{a:?}");
    assert!(!folder.exists());
    assert!(!tmp.exists());
    let installed = install_ready(&agents, &events, &agent_a());
    assert_eq!(installed.folder, folder);
    assert!(folder.join("clauseo-receipt.json").is_file());
    assert!(!folder.join("node_modules/adapter-a/stale.js").exists());

    // A fresh start finds the receipt: Ready at once, with no request.
    drop(agents);
    let before = fx.registry.requests().len();
    let (again, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Ready { .. }), "{a:?}");
    assert_eq!(again.installed(&agent_a()), Some(installed));
    assert_eq!(fx.registry.requests().len(), before);
}

/// A receipt naming a Node other than the one the `PATH` search finds now (an nvm switch, or a
/// planted path) is never run: the receipt is rewritten to name the Node found now and the agent
/// stays `Ready`, with no download. With no usable Node at all, the install is redone.
#[cfg(unix)]
#[test]
fn a_receipt_naming_another_node_is_rewritten_to_the_one_found_now() {
    let mut fx = Fixture::new("other-node");
    let (agents, events) = fx.start();
    events.first_states();
    let installed = install_ready(&agents, &events, &agent_a());
    drop(agents);
    let elsewhere = fx.data.path().join("elsewhere");
    let planted = elsewhere.join("bin/node");
    let marker = fx.data.path().join("planted-ran");
    let script = format!("#!/bin/sh\ntouch '{}'\necho v24.0.0\n", marker.display());
    write::write_executable(&planted, script).unwrap();
    write::write(&elsewhere.join("lib/node_modules/npm/bin/npm-cli.js"), "").unwrap();
    let receipt_path = installed.folder.join("clauseo-receipt.json");
    let read = || -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap()
    };
    let mut receipt = read();
    receipt["node"] = planted.display().to_string().into();
    write::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();

    let before = fx.registry.requests().len();
    let (again, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Ready { .. }), "{a:?}");
    assert_eq!(again.installed(&agent_a()).unwrap().node, installed.node);
    assert_eq!(read()["node"], installed.node.display().to_string());
    assert!(!marker.exists(), "the planted Node never ran");
    assert_eq!(fx.registry.requests().len(), before);
    drop(again);

    // No Node on PATH and no runtime: nothing usable, so the agent is Missing.
    fx.config.node_search = clauseo_agents::NodeSearch::none();
    let (_again, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Missing { .. }), "{a:?}");
}

#[test]
fn no_install_folder_holds_a_link() {
    let fx = Fixture::new("links");
    let (agents, events) = fx.start();
    events.first_states();
    let a = install_ready(&agents, &events, &agent_a());
    let b = install_ready(&agents, &events, &agent_b());
    assert_eq!(
        links_under(&fx.data.path().join("agents")),
        Vec::<std::path::PathBuf>::new()
    );
    for installed in [a, b] {
        assert!(!installed.folder.join("node_modules/.bin").exists());
    }
    // After Ready the install's downloads and npm's cache are gone.
    let downloads = fx.data.path().join("agents/downloads");
    let left: Vec<_> = std::fs::read_dir(&downloads)
        .map(|dir| dir.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    assert_eq!(left, Vec::<std::path::PathBuf>::new());
    assert!(!fx.data.path().join("agents/npm-cache").exists());
}

#[cfg(unix)]
mod node_20 {
    use std::io::Write as _;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::install_ready;
    use std::time::Duration;

    use crate::support::{
        A_PACKAGES, BASE, Fixture, NODE_VERSION, SharedTimer, agent_a, agent_b, node_entry,
        node_path, platform, real_node, snapshot,
    };
    use crate::write;
    use clauseo_agents::InstallState;

    /// A Node archive wrapping the real Node: `bin/node` a script that execs it, and an
    /// `npm-cli.js` that requires the real one.
    fn wrapper_archive() -> Vec<u8> {
        let (program, cli) = real_node();
        let top = format!("node-v{NODE_VERSION}-{}", platform().node_name());
        let node = format!("#!/bin/sh\nexec '{}' \"$@\"\n", program.display());
        let npm = format!(
            "require({});\n",
            serde_json::to_string(&cli.display().to_string()).unwrap()
        );
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        for (path, bytes, mode) in [
            ("bin/node", node.as_bytes(), 0o755),
            ("lib/node_modules/npm/bin/npm-cli.js", npm.as_bytes(), 0o644),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(u64::try_from(bytes.len()).unwrap());
            header.set_mode(mode);
            header.set_mtime(0);
            tar.append_data(&mut header, format!("{top}/{path}"), bytes)
                .unwrap();
        }
        let mut gz = tar.into_inner().unwrap();
        gz.flush().unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn a_node_20_on_path_is_not_used() {
        let mut fx = Fixture::new("node20");
        let fake = fx.data.path().join("fake-node20");
        write::write_executable(&fake.join("bin/node"), "#!/bin/sh\necho v20.11.0\n").unwrap();
        write::write(&fake.join("lib/node_modules/npm/bin/npm-cli.js"), "").unwrap();
        fx.config.node_search.path = Some(fake.join("bin").into_os_string());
        let archive = wrapper_archive();
        let entry = node_entry(Some(&archive));
        fx.registry.route(&node_path(&entry), archive.clone());
        fx.config.snapshot = snapshot(BASE, entry, 10_000);

        let (agents, events) = fx.start();
        let (a, _) = events.first_states();
        let packages: usize = A_PACKAGES.iter().map(|n| fx.tarballs[n].len()).sum();
        let want = u64::try_from(packages + archive.len()).unwrap();
        assert_eq!(
            a,
            InstallState::Missing {
                download_bytes: want
            }
        );

        let installed = install_ready(&agents, &events, &agent_a());
        let runtime = fx
            .data
            .path()
            .join("agents/runtime")
            .join(format!("node-v{NODE_VERSION}-{}", platform().node_name()));
        assert_eq!(installed.node, runtime.join("bin/node"));
        let receipt = std::fs::read(installed.folder.join("clauseo-receipt.json")).unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&receipt).unwrap();
        assert_eq!(
            receipt["node"],
            runtime.join("bin/node").display().to_string()
        );
        assert_eq!(receipt["node_version"], format!("v{NODE_VERSION}"));
        assert!(runtime.join("clauseo-runtime.json").is_file());
        assert!(
            fx.registry
                .requests()
                .iter()
                .any(|r| r.path == node_path_of(&fx))
        );
    }

    /// With an offline bundle, Node's archive is copied into `downloads/` as it is hashed and
    /// only the copy is unpacked; nothing touches the network, and the bundle stays as it was.
    #[test]
    fn a_bundled_node_installs_from_its_copy_with_no_network_call() {
        use sha2::Digest as _;
        let mut fx = Fixture::new("bundle-node");
        fx.config.node_search = clauseo_agents::NodeSearch::none();
        let archive = wrapper_archive();
        let entry = node_entry(Some(&archive));
        let name = entry.archive(platform()).unwrap().name().to_owned();
        fx.config.snapshot = snapshot(BASE, entry, 10_000);
        let bundle = clauseo_agents::testing::TempDir::new("bundle-node-files").unwrap();
        write::write(&bundle.path().join("node").join(&name), &archive).unwrap();
        for package in A_PACKAGES {
            let bytes = &fx.tarballs[package];
            let hex = crate::support::to_hex(&sha2::Sha512::digest(bytes));
            write::write(&bundle.path().join("npm").join(format!("{hex}.tgz")), bytes).unwrap();
        }
        fx.config.install.bundle = Some(bundle.path().to_owned());

        let (agents, events) = fx.start();
        events.first_states();
        let installed = install_ready(&agents, &events, &agent_a());
        let runtime = fx
            .data
            .path()
            .join("agents/runtime")
            .join(format!("node-v{NODE_VERSION}-{}", platform().node_name()));
        assert_eq!(installed.node, runtime.join("bin/node"));
        assert!(runtime.join("clauseo-runtime.json").is_file());
        assert_eq!(fx.lookups.names(), Vec::<String>::new());
        assert_eq!(fx.registry.requests(), []);
        // The copy is gone after Ready; the bundle's archive stays.
        assert!(!fx.data.path().join("agents/downloads").join(&name).exists());
        assert_eq!(
            std::fs::read(bundle.path().join("node").join(&name)).unwrap(),
            archive
        );
    }

    /// A runtime whose `node` no longer answers `--version` (cut short by a power cut, say) is
    /// not trusted at start: the agent is `Missing`, and an install fetches Node again.
    #[test]
    fn a_runtime_node_that_does_not_run_is_fetched_again() {
        let mut fx = Fixture::new("broken-runtime");
        fx.config.node_search = clauseo_agents::NodeSearch::none();
        let archive = wrapper_archive();
        let entry = node_entry(Some(&archive));
        fx.registry.route(&node_path(&entry), archive);
        fx.config.snapshot = snapshot(BASE, entry, 10_000);
        let (agents, events) = fx.start();
        events.first_states();
        let installed = install_ready(&agents, &events, &agent_a());
        drop(agents);
        write::write_executable(&installed.node, "#!/bin/sh\nexit 1\n").unwrap();

        let (again, events) = fx.start();
        let (a, _) = events.first_states();
        assert!(matches!(a, InstallState::Missing { .. }), "{a:?}");
        let redone = install_ready(&again, &events, &agent_a());
        assert_eq!(redone.node, installed.node);
        let node_requests = fx
            .registry
            .requests()
            .iter()
            .filter(|r| r.path == node_path_of(&fx))
            .count();
        assert_eq!(node_requests, 2);
    }

    /// A runtime whose `--version` takes past its 10 s limit at start is not proof of a bad
    /// Node: the agent stays `Ready`, with no download.
    #[test]
    fn a_runtime_check_that_times_out_keeps_the_agent_ready() {
        let mut fx = Fixture::new("slow-runtime");
        fx.config.node_search = clauseo_agents::NodeSearch::none();
        let archive = wrapper_archive();
        let entry = node_entry(Some(&archive));
        fx.registry.route(&node_path(&entry), archive);
        fx.config.snapshot = snapshot(BASE, entry, 10_000);
        let (agents, events) = fx.start();
        events.first_states();
        let installed = install_ready(&agents, &events, &agent_a());
        drop(agents);
        let real = std::fs::read(&installed.node).unwrap();
        // It never answers (the launcher's handle kills it when the wait ends).
        write::write_executable(&installed.node, "#!/bin/sh\nexec tail -f /dev/null\n").unwrap();
        fx.timer = SharedTimer::default();
        let before = fx.registry.requests().len();
        let (again, events) = fx.start();
        fx.timer.fire_next(Duration::from_secs(10));
        let (a, _) = events.first_states();
        assert!(matches!(a, InstallState::Ready { .. }), "{a:?}");
        assert_eq!(again.installed(&agent_a()).unwrap().node, installed.node);
        assert_eq!(fx.registry.requests().len(), before);
        write::write_executable(&installed.node, real).unwrap();
    }

    /// An npm in the runtime that cannot load its own files has the runtime removed; the next
    /// try fetches Node again and installs.
    #[test]
    fn a_runtime_npm_missing_its_modules_is_fetched_again() {
        let mut fx = Fixture::new("broken-npm");
        fx.config.node_search = clauseo_agents::NodeSearch::none();
        let archive = wrapper_archive();
        let entry = node_entry(Some(&archive));
        fx.registry.route(&node_path(&entry), archive);
        fx.config.snapshot = snapshot(BASE, entry, 10_000);
        let (agents, events) = fx.start();
        events.first_states();
        let installed = install_ready(&agents, &events, &agent_a());
        let runtime = installed
            .node
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        let cli = runtime.join("lib/node_modules/npm/bin/npm-cli.js");
        write::write(&cli, "require('./no-such-npm-file');\n").unwrap();

        agents.install(&agent_b());
        fx.timer.fire_next(Duration::from_secs(2));
        let (_, last) = events.until_end(&agent_b());
        assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
        let node_requests = fx
            .registry
            .requests()
            .iter()
            .filter(|r| r.path == node_path_of(&fx))
            .count();
        assert_eq!(node_requests, 2);
        assert_ne!(
            std::fs::read_to_string(&cli).unwrap(),
            "require('./no-such-npm-file');\n"
        );
    }

    fn node_path_of(fx: &Fixture) -> String {
        node_path(fx.config.snapshot.node())
    }
}
