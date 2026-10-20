//! Tests of how the YAML reader finds comments and follows merge keys.

use super::*;

/// A quoted scalar ends at its own closing quote, so a `#` inside it is text and a `#` after it
/// starts a comment. Single quotes escape with `''` and take `\` as a plain character; double
/// quotes take `\` as an escape.
#[test]
fn a_quoted_scalar_ends_at_its_own_closing_quote() {
    let text = concat!(
        "a: 'q # not' # one\n",
        "b: 'it''s # not' # two\n",
        "c: \"d \\\" # not\" # three\n",
        "d: 'end\\' # four\n",
    );
    let doc = yaml::parse(text).expect("valid YAML");
    assert_eq!(
        doc.comments(text),
        [(1, " one"), (2, " two"), (3, " three"), (4, " four")]
    );
}

/// The keys merged into the mapping `b<depth>`: a chain of `depth` merges ends at `b0`, whose
/// `k` the chain brings in.
fn merged_keys(depth: usize) -> Vec<String> {
    let mut lines = vec!["b0: &b0 {k: v}".to_owned()];
    for n in 1..=depth {
        lines.push(format!("b{n}: &b{n} {{<<: *b{}}}", n - 1));
    }
    let text = lines.join("\n") + "\n";
    let doc = yaml::parse(&text).expect("valid YAML");
    let [root] = doc.roots() else {
        panic!("one document");
    };
    let name = format!("b{depth}");
    let (_, end) = root
        .entries()
        .into_iter()
        .find(|(key, _)| key.scalar() == Some(name.as_str()))
        .expect("the chain's end");
    end.entries()
        .iter()
        .filter_map(|(key, _)| key.scalar().map(str::to_owned))
        .collect()
}

/// Merges are followed to depth 64 and no further: `k` comes through a chain of 64 merges and
/// not through one of 65.
#[test]
fn merge_keys_are_followed_to_depth_64_and_no_further() {
    assert!(merged_keys(64).contains(&"k".to_owned()));
    assert!(!merged_keys(65).contains(&"k".to_owned()));
}
