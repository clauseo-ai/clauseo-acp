//! The two versions' own views compared, on stetdoc 0.9.3's real output (`partial/final-*.json`:
//! the bank-details pair whose link's address alone changes, a comment's text, a text box's
//! text) and on lines made for the cases around them.

use super::{Crossed, LINK_TEXT_MAX, Side, cross, read_side};

/// A fixture of `tests/fixtures/blackline/partial/`, as text.
macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/blackline/partial/",
            $name
        ))
    };
}

/// A version's view whose text is `lines`, after a header.
fn final_view(lines: &str) -> String {
    serde_json::json!({
        "mode": "final",
        "text": format!("{{stet 1 view=body mode=final}}\n{{summary: 1 section}}\n{lines}\n"),
        "next": null,
    })
    .to_string()
}

/// The side `lines` make.
fn side(lines: &str) -> Side {
    read_side(&final_view(lines)).unwrap()
}

/// What differs between the sides `old` and `new` make.
fn crossed(old: &str, new: &str) -> Crossed {
    cross(&side(old), &side(new))
}

/// The bank-details pair: "the account" points at acct-111, then acct-999, and nothing else
/// changes. The link is named; the same version on both sides gives nothing.
#[test]
fn the_bank_details_link_is_named() {
    let old = read_side(fixture!("final-link-old.json")).unwrap();
    let new = read_side(fixture!("final-link-new.json")).unwrap();
    assert_eq!(
        new.links,
        [(
            "the account".to_owned(),
            "https://bank.example/acct-999".to_owned()
        )]
    );
    assert_eq!(
        cross(&old, &new),
        Crossed {
            links: vec!["the account".to_owned()],
            ..Crossed::default()
        }
    );
    assert_eq!(cross(&new, &new), Crossed::default());
}

#[test]
fn a_comment_s_new_text_is_found() {
    let old = read_side(fixture!("final-comment-old.json")).unwrap();
    let new = read_side(fixture!("final-comment-new.json")).unwrap();
    assert_eq!(new.comments, ["The client agreed the fee."]);
    let found = cross(&old, &new);
    assert!(found.comments && !found.boxes && found.links.is_empty());
}

#[test]
fn a_text_box_s_new_text_is_found() {
    let old = read_side(fixture!("final-textbox-old.json")).unwrap();
    let new = read_side(fixture!("final-textbox-new.json")).unwrap();
    assert_eq!(new.boxes, ["Cap of 2 million."]);
    let found = cross(&old, &new);
    assert!(found.boxes && !found.comments);
}

/// A link added to kept text or taken off it is named, and so is a new link around new text (its
/// address is new); a link that moves with its text is not; a link with no text can't be named.
#[test]
fn links_are_compared_by_text_and_address() {
    let linked = "p1 Pay to {link \"https://a.example\"}the account{/link}.";
    let plain = "p1 Pay to the account.";
    let named = |names: &[&str]| Crossed {
        links: names.iter().map(|name| (*name).to_owned()).collect(),
        ..Crossed::default()
    };
    assert_eq!(crossed(plain, linked), named(&["the account"]));
    assert_eq!(crossed(linked, plain), named(&["the account"]));
    let new_text = "p1 Pay to {link \"https://a.example\"}the new account{/link}.";
    assert_eq!(crossed(plain, new_text), named(&["the new account"]));
    let two = "p1 {link \"https://b.example\"}B{/link}\np2 {link \"https://a.example\"}A{/link}";
    let swapped =
        "p1 {link \"https://a.example\"}A{/link}\np2 {link \"https://b.example\"}B{/link}";
    assert_eq!(crossed(two, swapped), Crossed::default());
    let picture = |to: &str| format!("p1 {{link \"{to}\"}}{{img \"logo\"}}{{/link}}");
    let unnamed = crossed(&picture("https://a.example"), &picture("https://b.example"));
    assert!(unnamed.unnamed_link && unnamed.links.is_empty());
    // A place in the document (`->p57`) moves when a paragraph comes before it: not a change.
    assert_eq!(
        crossed(
            "p1 {link ->p57}clause 4{/link}",
            "p1 {link ->p58}clause 4{/link}"
        ),
        Crossed::default()
    );
}

/// "account A" at one address becomes "account B" at another: neither text is on both sides,
/// but the address is new, so the link is named (the new one first, then the one it replaced).
#[test]
fn a_link_whose_text_and_address_both_change_is_named() {
    let found = crossed(
        "p1 Pay to {link \"https://bank.example/1\"}account A{/link}.",
        "p1 Pay to {link \"https://bank.example/2\"}account B{/link}.",
    );
    assert_eq!(
        found.links,
        ["account B".to_owned(), "account A".to_owned()]
    );
    assert!(!found.unnamed_link);
}

/// Two links of one text swap addresses: the addresses are the same on both sides, but each
/// link's changed, so the text is named.
#[test]
fn links_of_one_text_that_swap_addresses_are_named() {
    let lines = |first: &str, second: &str| {
        format!(
            "p1 Pay {{link \"https://bank.example/{first}\"}}here{{/link}}.\n\
             p2 Bill {{link \"https://bank.example/{second}\"}}here{{/link}}."
        )
    };
    let found = crossed(&lines("pay", "bill"), &lines("bill", "pay"));
    assert_eq!(found.links, ["here".to_owned()]);
    assert_eq!(
        crossed(&lines("pay", "bill"), &lines("pay", "bill")),
        Crossed::default()
    );
}

/// Each changed link once, the newer version's order first, its text with the escapes read and
/// cut to the cap.
#[test]
fn link_texts_are_named_once_and_cut() {
    let long = "x".repeat(LINK_TEXT_MAX + 5);
    let lines = |to: &str| {
        format!(
            "p1 {{link \"{to}\"}}B \\{{1\\}}{{/link}} {{link \"{to}\"}}A{{/link}}\n\
             p2 {{link \"{to}\"}}{long}{{/link}} {{link \"{to}\"}}A{{/link}}"
        )
    };
    let found = crossed(&lines("https://old.example"), &lines("https://new.example"));
    let cut = format!("{}…", "x".repeat(LINK_TEXT_MAX));
    assert_eq!(found.links, ["B {1}".to_owned(), "A".to_owned(), cut]);
}

#[test]
fn a_view_that_is_not_a_whole_final_one_does_not_read() {
    let view = |mode: &str, next: serde_json::Value, text: &str| {
        serde_json::json!({"mode": mode, "text": text, "next": next}).to_string()
    };
    let header = "{stet 1 view=body mode=final}\np1 Text.\n";
    assert!(read_side(&view("final", serde_json::Value::Null, header)).is_ok());
    assert!(read_side(&view("redline", serde_json::Value::Null, header)).is_err());
    assert!(read_side(&view("final", "after p1".into(), header)).is_err());
    assert!(
        read_side(&view(
            "final",
            serde_json::Value::Null,
            "{stet 2}\np1 Text."
        ))
        .is_err()
    );
    assert!(read_side(&final_view("p1 a {link \"x\"} b } c")).is_err());
}
