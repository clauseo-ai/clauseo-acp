//! The cache keys, the shipped copies and their derivation, and the cache file's bounds.

use clauseo_connectors::fake::ScratchDir;
use clauseo_connectors::{
    CASE_LAW_INITIALIZE, CASE_LAW_TOOLS_LIST, CASE_LAW_TOOLS_LIST_2025_03_26,
};

use super::*;

fn dev() -> Origin {
    Origin::Fake(url::Url::parse("http://127.0.0.1:8793").unwrap())
}

#[test]
fn each_origin_and_version_has_its_own_cache_file() {
    let dir = Path::new("cache");
    let production = cache_path(dir, &Origin::Production, "2025-06-18");
    let dev_path = cache_path(dir, &dev(), "2025-06-18");
    let other = cache_path(
        dir,
        &Origin::Fake(url::Url::parse("http://127.0.0.1:9000").unwrap()),
        "2025-06-18",
    );
    let older = cache_path(dir, &Origin::Production, "2025-03-26");
    assert_ne!(production, dev_path);
    assert_ne!(dev_path, other);
    assert_ne!(production, older);
    assert_eq!(
        production,
        cache_path(dir, &Origin::Production, "2025-06-18")
    );
    assert_eq!(production.parent(), Some(dir));
}

#[test]
fn the_shipped_copies_derive_the_servers_exact_bytes() {
    let text = |value: &Value| serde_json::to_string(value).unwrap();
    let production = Origin::Production;
    let june = shipped("2025-06-18", &production);
    assert_eq!(text(&june.initialize), CASE_LAW_INITIALIZE);
    assert_eq!(text(&june.tools_list), CASE_LAW_TOOLS_LIST);
    let november = shipped("2025-11-25", &production);
    assert_eq!(
        text(&november.initialize),
        CASE_LAW_INITIALIZE.replace(
            r#""protocolVersion":"2025-06-18""#,
            r#""protocolVersion":"2025-11-25""#
        )
    );
    assert_eq!(text(&november.tools_list), CASE_LAW_TOOLS_LIST);
    let march = shipped("2025-03-26", &production);
    let title = r#""title":"Clauseo Case Law","#;
    assert!(CASE_LAW_INITIALIZE.contains(title));
    assert_eq!(
        text(&march.initialize),
        CASE_LAW_INITIALIZE
            .replace(
                r#""protocolVersion":"2025-06-18""#,
                r#""protocolVersion":"2025-03-26""#
            )
            .replace(title, "")
    );
    assert_eq!(text(&march.tools_list), CASE_LAW_TOOLS_LIST_2025_03_26);
    for version in INITIALIZE_VERSIONS {
        let copy = shipped(version, &production);
        assert!(usable(&copy.initialize, &copy.tools_list), "{version}");
    }
}

#[test]
fn the_shipped_instructions_name_the_origins_base_url() {
    let copy = shipped("2025-06-18", &dev());
    let instructions = copy.initialize["instructions"].as_str().unwrap();
    assert!(
        instructions.contains("http://127.0.0.1:8793/pay/"),
        "{instructions}"
    );
    assert!(!instructions.contains(BASE_PLACEHOLDER), "{instructions}");
}

#[test]
fn a_dev_origins_copy_never_serves_production_nor_another_version() {
    let scratch = ScratchDir::new("copies").unwrap();
    let dir = scratch.path().join("cache");
    let dev_copies = Copies::load(&dir, &dev());
    let mut initialize = (*dev_copies.current("2025-06-18").initialize).clone();
    initialize["instructions"] = json!("Payment links always start http://127.0.0.1:8793/pay/.");
    let served = Served {
        initialize: Arc::new(initialize.clone()),
        tools_list: dev_copies.current("2025-06-18").tools_list,
    };
    dev_copies.store("2025-06-18", served, 1);
    // The same origin after a restart reads it back; another version does not.
    let again = Copies::load(&dir, &dev());
    assert_eq!(*again.current("2025-06-18").initialize, initialize);
    assert_eq!(again.current("2025-03-26"), shipped("2025-03-26", &dev()));
    // Production, in the same folder, keeps its shipped copy.
    let production = Copies::load(&dir, &Origin::Production);
    assert_eq!(
        production.current("2025-06-18"),
        shipped("2025-06-18", &Origin::Production)
    );
}

#[test]
fn usable_lists_have_named_tools() {
    assert!(!usable(
        &json!({}),
        &json!({"tools": [], "nextCursor": "2"})
    ));
    assert!(!usable(
        &json!([]),
        &json!({"tools": [{"name": "a", "inputSchema": {}}]})
    ));
    assert!(!usable(&json!({}), &json!({"tools": []})));
    assert!(!usable(
        &json!({}),
        &json!({"tools": [{"title": "no name"}]})
    ));
    assert!(!usable(&json!({}), &json!({"tools": [{"name": "a"}]})));
    assert!(!usable(
        &json!({}),
        &json!({"tools": [{"name": "a", "inputSchema": "x"}]})
    ));
    assert!(usable(
        &json!({}),
        &json!({"tools": [{"name": "a", "inputSchema": {"type": "object"}}]})
    ));
}
