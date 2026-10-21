//! Tests of how `web.rs` reads the Node range from `engines.node` in `ui/package.json`, for the
//! fix when `node` cannot run.

use super::*;

#[test]
fn the_node_fix_reads_engines_node_from_ui_package_json() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let text = std::fs::read_to_string(root.join("ui/package.json")).expect("read ui/package.json");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("ui/package.json is JSON");
    let range = manifest["engines"]["node"]
        .as_str()
        .expect("ui/package.json has engines.node");
    assert_eq!(node_engines(&root).as_deref(), Some(range));
}

#[test]
fn the_node_fix_names_the_range_or_the_field_it_could_not_read() {
    let dir = Scratch::new("web-node-engines");
    assert_eq!(node_engines(&dir), None);
    Scratch::create_dir_all(dir.join("ui")).expect("create ui/");
    for text in ["{", r#"{"engines": {}}"#, r#"{"engines": {"node": 20}}"#] {
        Scratch::write(dir.join("ui/package.json"), text).expect("write ui/package.json");
        assert_eq!(node_engines(&dir), None, "{text}");
    }
    Scratch::write(
        dir.join("ui/package.json"),
        r#"{"engines": {"node": ">=20.5.0"}}"#,
    )
    .expect("write ui/package.json");
    let range = node_engines(&dir);
    assert_eq!(range.as_deref(), Some(">=20.5.0"));
    assert_eq!(
        install_node(range.as_deref()),
        "install a Node that matches >=20.5.0 (`engines.node` in ui/package.json)"
    );
    assert_eq!(
        install_node(None),
        "install the Node that `engines.node` in ui/package.json names"
    );
}
