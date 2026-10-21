//! Tests of how `web.rs` reads the minimum Node from `engines.node` in `ui/package.json`, with the
//! grammar `checks::pins` writes down, for the fix when `node` cannot run.

use super::*;

#[test]
fn the_node_fix_reads_engines_node_from_ui_package_json() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let text = std::fs::read_to_string(root.join("ui/package.json")).expect("read ui/package.json");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("ui/package.json is JSON");
    let range = manifest["engines"]["node"]
        .as_str()
        .expect("ui/package.json has engines.node");
    let min = min_node(&root).expect("engines.node takes the one form");
    assert_eq!(format!(">={min}"), range);
}

/// The fix names the minimum when `engines.node` has the one form it takes, and the rule when
/// the file, its JSON or the field cannot be read or has another form.
#[test]
fn the_node_fix_names_the_minimum_or_the_rule() {
    let dir = Scratch::new("web-node-engines");
    assert_eq!(min_node(&dir), None);
    Scratch::create_dir_all(dir.join("ui")).expect("create ui/");
    for text in [
        "{",
        r#"{"engines": {}}"#,
        r#"{"engines": {"node": 20}}"#,
        r#"{"engines": {"node": "^20.5.0"}}"#,
        r#"{"engines": {"node": ">= 20.5.0"}}"#,
        r#"{"engines": {"node": ">=20.5.0 <30"}}"#,
    ] {
        Scratch::write(dir.join("ui/package.json"), text).expect("write ui/package.json");
        assert_eq!(min_node(&dir), None, "{text}");
    }
    Scratch::write(
        dir.join("ui/package.json"),
        r#"{"engines": {"node": ">=20.5.0"}}"#,
    )
    .expect("write ui/package.json");
    let min = min_node(&dir);
    assert_eq!(min.as_deref(), Some("20.5.0"));
    assert_eq!(
        install_node(min.as_deref()),
        "install Node 20.5.0 or later (`engines.node` in ui/package.json)"
    );
    assert_eq!(
        install_node(None),
        "set `engines.node` in ui/package.json to the minimum Node (engines.node takes only \
         \">=<major>.<minor>.<patch>\"), then install that Node or a later one"
    );
}
