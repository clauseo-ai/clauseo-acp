use super::*;

const REGISTRY: &str = "https://registry.npmjs.org/";
const SHA: &str = "sha512-t1k8YPDvo/zUghL3x2pFs40tWfYTLhYmK8jt37CVBpY7eq1Ztj2hLZBAmy9ZX++tUyJaGfuuMrzvJGvU9tjBsg==";

fn manifest() -> String {
    r#"{"name":"fixture","version":"0.0.0","private":true,
        "dependencies":{"a":"1.0.0","b":"1.0.0"},"overrides":{"c":"2.0.0","@s/z@1":"1.0.0"}}"#
        .to_owned()
}

/// A lock where `a` needs `c` (nested under `a` at 2.0.0, top level at 1.0.0), `c` peers
/// optionally on `gone` and on `d`, and `b` needs `e`. `alias` is `@s/z` under another path.
fn lock(reversed: bool) -> String {
    let entry = |path: &str, extra: &str| {
        format!(
            r#""{path}":{{"version":"1.0.0","resolved":"{REGISTRY}{path}.tgz","integrity":"{SHA}"{extra}}}"#
        )
    };
    let mut entries = [
        r#""":{"name":"fixture","version":"0.0.0","dependencies":{"a":"1.0.0","b":"1.0.0"}}"#
            .to_owned(),
        entry(
            "node_modules/a",
            r#","dependencies":{"c":"2"},"optionalDependencies":{"alias":"1"}"#,
        ),
        entry(
            "node_modules/a/node_modules/c",
            r#","peerDependencies":{"gone":"1","d":"1"},"peerDependenciesMeta":{"gone":{"optional":true}}"#,
        ),
        entry("node_modules/c", ""),
        entry(
            "node_modules/d",
            r#","os":["linux"],"cpu":["x64"],"optional":true"#,
        ),
        entry("node_modules/alias", r#","name":"@s/z""#),
        entry("node_modules/b", r#","dependencies":{"e":"1"}"#),
        entry("node_modules/e", ""),
    ];
    if reversed {
        entries.reverse();
    }
    let packages = entries.join(",");
    if reversed {
        format!(
            r#"{{"packages":{{{packages}}},"requires":true,"lockfileVersion":3,"name":"fixture"}}"#
        )
    } else {
        format!(
            r#"{{"name":"fixture","lockfileVersion":3,"requires":true,"packages":{{{packages}}}}}"#
        )
    }
}

fn paths(pins: &AgentPins) -> Vec<&str> {
    pins.packages().iter().map(|p| p.path.as_str()).collect()
}

#[test]
fn dependencies_resolve_nested_first_then_upward_and_peers_are_followed() {
    let pins = derive(&manifest(), &lock(false), "a", REGISTRY).unwrap();
    assert_eq!(
        paths(&pins),
        [
            "node_modules/a",
            "node_modules/a/node_modules/c",
            "node_modules/alias",
            "node_modules/d",
        ]
    );
    let alias = &pins.packages()[2];
    assert_eq!(alias.name, "@s/z");
    let b = derive(&manifest(), &lock(false), "b", REGISTRY).unwrap();
    assert_eq!(paths(&b), ["node_modules/b", "node_modules/e"]);
}

#[test]
fn the_bytes_are_the_same_whichever_key_order_the_input_had() {
    let one = derive(&manifest(), &lock(false), "a", REGISTRY).unwrap();
    let two = derive(&manifest(), &lock(true), "a", REGISTRY).unwrap();
    assert_eq!(one.lock(), two.lock());
    assert_eq!(one.package_json(), two.package_json());
    assert!(one.lock().ends_with("}\n"));
    assert!(
        one.lock()
            .starts_with("{\n  \"lockfileVersion\": 3,\n  \"name\": \"fixture\",\n")
    );
}

#[test]
fn canonical_json_sorts_keys_and_indents_by_two() {
    let value: Value = serde_json::from_str(r#"{"b":[1,{"y":{},"x":[]}],"a":"é\""}"#).unwrap();
    assert_eq!(
        canonical(&value),
        "{\n  \"a\": \"é\\\"\",\n  \"b\": [\n    1,\n    {\n      \"x\": [],\n      \"y\": {}\n    }\n  ]\n}\n"
    );
}

#[test]
fn overrides_go_only_to_the_agent_whose_lock_holds_the_package() {
    let a = derive(&manifest(), &lock(false), "a", REGISTRY).unwrap();
    let manifest_a: Value = serde_json::from_str(a.package_json()).unwrap();
    assert_eq!(manifest_a["overrides"]["c"], "2.0.0");
    assert_eq!(manifest_a["overrides"]["@s/z@1"], "1.0.0");
    assert_eq!(
        manifest_a["dependencies"],
        serde_json::json!({"a": "1.0.0"})
    );
    let b = derive(&manifest(), &lock(false), "b", REGISTRY).unwrap();
    assert!(!b.package_json().contains("overrides"));
    let lock_b: Value = serde_json::from_str(b.lock()).unwrap();
    assert_eq!(
        lock_b["packages"][""]["dependencies"],
        serde_json::json!({"b": "1.0.0"})
    );
}

#[test]
fn a_missing_dependency_that_is_not_an_optional_peer_fails() {
    let broken = lock(false).replace(r#""node_modules/e":"#, r#""node_modules/other":"#);
    let err = derive(&manifest(), &broken, "b", REGISTRY).unwrap_err();
    assert_eq!(
        err,
        PinsError::Missing {
            from: "node_modules/b".into(),
            name: "e".into()
        }
    );
    let no_peer = lock(false).replace(r#""node_modules/d":"#, r#""node_modules/other":"#);
    assert!(matches!(
        derive(&manifest(), &no_peer, "a", REGISTRY),
        Err(PinsError::Missing { name, .. }) if name == "d"
    ));
}

#[test]
fn an_unknown_root_a_foreign_url_and_a_weak_integrity_are_refused() {
    assert_eq!(
        derive(&manifest(), &lock(false), "zz", REGISTRY),
        Err(PinsError::UnknownRoot("zz".into()))
    );
    let foreign = lock(false).replace(
        &format!("{REGISTRY}node_modules/e.tgz"),
        "https://evil.example/e.tgz",
    );
    assert!(matches!(
        derive(&manifest(), &foreign, "b", REGISTRY),
        Err(PinsError::Resolved { path, .. }) if path == "node_modules/e"
    ));
    let weak = lock(false).replacen(SHA, "sha1-AAAA", 8);
    assert!(matches!(
        derive(&manifest(), &weak, "b", REGISTRY),
        Err(PinsError::Integrity { .. })
    ));
}

#[test]
fn the_platform_filter_follows_os_and_cpu() {
    let pins = derive(&manifest(), &lock(false), "a", REGISTRY).unwrap();
    let linux = Platform::new(crate::platform::Os::Linux, crate::platform::Arch::X64);
    let mac = Platform::new(crate::platform::Os::Darwin, crate::platform::Arch::Arm64);
    assert_eq!(pins.packages_for(linux).len(), 4);
    assert_eq!(pins.packages_for(mac).len(), 3);
    assert!(pins.packages()[3].optional);
}

#[test]
fn override_names_drop_their_range() {
    assert_eq!(override_name("c"), "c");
    assert_eq!(override_name("c@^1"), "c");
    assert_eq!(override_name("@openai/codex"), "@openai/codex");
    assert_eq!(override_name("@openai/codex@0.1"), "@openai/codex");
}

#[test]
fn file_urls_have_three_slashes_forward_slashes_and_no_percent_encoding() {
    assert_eq!(
        file_url("/home/u/data dir/a#b%c.tgz", Os::Linux),
        "file:///home/u/data dir/a#b%c.tgz"
    );
    assert_eq!(
        file_url("/Users/u/a.tgz", Os::Darwin),
        "file:///Users/u/a.tgz"
    );
    // A backslash is a file name's character on Unix.
    assert_eq!(file_url(r"/tmp/a\b.tgz", Os::Linux), r"file:///tmp/a\b.tgz");
    assert_eq!(
        file_url(r"C:\Users\J S\AppData\Local\a.tgz", Os::Win32),
        "file:///C:/Users/J S/AppData/Local/a.tgz"
    );
    assert_eq!(
        file_url(r"\\?\D:\data\a.tgz", Os::Win32),
        "file:///D:/data/a.tgz"
    );
    assert_eq!(
        file_url(r"\\server\share\a.tgz", Os::Win32),
        "file:////server/share/a.tgz"
    );
    assert_eq!(
        file_url(r"\\?\UNC\server\share\a.tgz", Os::Win32),
        "file:////server/share/a.tgz"
    );
}

#[test]
fn the_install_lock_changes_only_the_mapped_resolved_urls() {
    let pins = derive(&manifest(), &lock(false), "a", REGISTRY).unwrap();
    let tarballs = BTreeMap::from([
        (
            "node_modules/a".to_owned(),
            PathBuf::from("/d/downloads/aa.tgz"),
        ),
        (
            "node_modules/a/node_modules/c".to_owned(),
            PathBuf::from("/bundle/npm/cc.tgz"),
        ),
    ]);
    let written = pins.lock_with_tarballs(&tarballs, Os::Linux).unwrap();
    let expected = pins
        .lock()
        .replace(
            &format!("\"{REGISTRY}node_modules/a.tgz\""),
            "\"file:///d/downloads/aa.tgz\"",
        )
        .replace(
            &format!("\"{REGISTRY}node_modules/a/node_modules/c.tgz\""),
            "\"file:///bundle/npm/cc.tgz\"",
        );
    assert_ne!(expected, pins.lock());
    assert_eq!(written, expected);
    assert_eq!(
        pins.lock_with_tarballs(&BTreeMap::new(), Os::Linux)
            .unwrap(),
        pins.lock()
    );
}

#[test]
fn the_install_lock_refuses_a_path_it_does_not_hold() {
    let pins = derive(&manifest(), &lock(false), "a", REGISTRY).unwrap();
    for path in ["node_modules/b", ""] {
        let tarballs = BTreeMap::from([(path.to_owned(), PathBuf::from("/d/b.tgz"))]);
        assert!(matches!(
            pins.lock_with_tarballs(&tarballs, Os::Linux),
            Err(PinsError::Malformed(_))
        ));
    }
}
