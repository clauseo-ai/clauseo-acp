use super::{literal, long_prefix, pandoc_reads, plain, short_has};
use crate::pre_write_guard::words::Escape;

fn args(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

/// Review round 3, finding 6: pandoc's parser takes any prefix of a long option and joined
/// short flags, so each spelling of an output is read, and options that run code refuse.
#[test]
fn pandoc_outputs_are_read_in_every_spelling() {
    let not_held = |out: &str| literal(out) && out != "Contract.docx";
    for text in [
        "x.md --out Contract.docx",
        "x.md --out=Contract.docx",
        "x.md --o Contract.docx",
        "x.md --output=Contract.docx",
        "x.md -so Contract.docx",
        "x.md -soContract.docx",
        "x.md -oContract.docx",
        "Contract.docx --lua-filter f.lua",
        "Contract.docx --lua f.lua",
        "Contract.docx --filter ./f",
        "Contract.docx --fil=./f",
        "Contract.docx -F ./f",
        "Contract.docx -L f.lua",
        "Contract.docx -d defaults.yaml",
        "Contract.docx -sd defaults.yaml",
        "Contract.docx --defaults=d.yaml",
        "Contract.docx --data-dir=d",
        "Contract.docx --extract-media=media",
        "Contract.docx --log=l.json",
        "Contract.docx --pdf-engine=./x",
        "x.md -o $out",
        "x.md -o Con*.docx",
    ] {
        assert!(!pandoc_reads(&args(text), not_held), "{text}");
    }
    for text in [
        "Contract.docx -t plain",
        "Contract.docx -o summary.md",
        "--output=out.txt Contract.docx",
        "-s Contract.docx -o -",
        "Contract.docx -t plain -- -o",
        "Contract.docx --to=markdown --wrap=none",
    ] {
        assert!(pandoc_reads(&args(text), not_held), "{text}");
    }
}

#[test]
fn substitutions_dollar_underscore_and_functions_are_not_plain() {
    for text in [
        "cat \"$(echo x)\"",
        "cat `echo x`",
        "cat <(echo x)",
        "head -c0 x; cp y \"$_\"",
        "cp y ${_}",
        "cat() { cp x \"$1\"; }; cat y",
        "cat ( ) { :; }",
    ] {
        assert!(!plain(text, Escape::Backslash), "{text}");
    }
    assert!(!plain("& { cat x }", Escape::Backtick));
    assert!(plain("cat x | grep y", Escape::Backslash));
    assert!(plain("echo `$x", Escape::Backtick));
}

#[test]
fn options_and_literals() {
    assert!(short_has("-ni", 'i') && !short_has("--in-place", 'i') && !short_has("x", 'i'));
    assert!(long_prefix("--in", "in-place") && long_prefix("--in-place=.bak", "in-place"));
    assert!(!long_prefix("--include", "in-place") && !long_prefix("--", "in-place"));
    assert!(literal("copy.txt") && literal("../out/a b.txt"));
    for word in ["", "~/x", "$f", "a*", "a?", "[ab]", "{a,b}", "`x`"] {
        assert!(!literal(word), "{word}");
    }
}
