//! Tests of how the YAML reader finds comments.

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
