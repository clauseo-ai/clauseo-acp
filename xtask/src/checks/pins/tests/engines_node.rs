//! The one grammar of `engines.node`, `>=<major>.<minor>.<patch>`, and the words its readers
//! fail with.

use super::*;

#[test]
fn engines_node_takes_only_greater_or_equal_and_three_numbers() {
    assert_eq!(engines_node_minimum(">=6.7.0"), Some("6.7.0"));
    assert_eq!(engines_node_minimum(">=10.20.30"), Some("10.20.30"));
    for field in [
        "^6.7.0",
        "~6.7.0",
        ">= 6.7.0",
        ">=6.7",
        ">=6",
        ">=6.7.0 <8",
        ">=6.7.0 || >=8.0.0",
        "6.7.0",
        ">=v6.7.0",
        ">=6.7.0.1",
        ">=6..0",
        ">=6.7.x",
        " >=6.7.0",
        "",
    ] {
        assert_eq!(engines_node_minimum(field), None, "{field}");
    }
}

/// A range in another form fails as a home that cannot be read, naming the grammar, and leaves
/// the rest of the check running.
#[test]
fn engines_node_in_another_form_fails_with_the_rule() {
    let dir = repo("node-caret");
    put(
        &dir,
        PACKAGE_JSON,
        "{ \"engines\": { \"node\": \"^6.7.0\" }, \"packageManager\": \"pnpm@7.8.9\" }",
    );
    let found = check(&dir);
    let printed: Vec<String> = found.failures.iter().map(ToString::to_string).collect();
    assert_eq!(
        printed,
        [
            "could not read the minimum Node from ui/package.json: `engines.node` is `^6.7.0`; \
             engines.node takes only \">=<major>.<minor>.<patch>\"\n    fix: set `\"engines\": \
             { \"node\": \">=<major>.<minor>.<patch>\" }` in ui/package.json: it is the one home \
             of the minimum Node, and pins reads it there"
        ]
    );
    assert_eq!(found.inspected, 1);
}

/// buildtime-lib.mjs, the Node reader of `engines.node`, fails in the same words.
#[test]
fn buildtime_lib_fails_in_the_words_of_the_rule() {
    let root = crate::workspace::workspace_root();
    let path = root.join("ui/app/scripts/buildtime-lib.mjs");
    let text = std::fs::read_to_string(&path).expect("read buildtime-lib.mjs");
    assert!(
        text.contains(&format!("const ENGINES_NODE_RULE = '{ENGINES_NODE_RULE}';")),
        "buildtime-lib.mjs does not define ENGINES_NODE_RULE as `{ENGINES_NODE_RULE}`"
    );
}
