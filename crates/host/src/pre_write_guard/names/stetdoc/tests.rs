use std::path::Path;

use clauseo_protocol::MatterPath;

use super::{Held, Stetdoc, parse, stetdoc};

fn held(names: &[&str]) -> Vec<Held> {
    names
        .iter()
        .map(|name| Held::new(MatterPath::from_relative(Path::new(name)).unwrap()))
        .collect()
}

fn words(command: &str) -> Vec<String> {
    command.split_whitespace().map(str::to_owned).collect()
}

/// Review round 3, finding 3: a flag's value is never the document, and `--in-place` with any
/// word naming a held file writes it.
#[test]
fn a_flag_value_is_never_taken_for_the_document() {
    let files = held(&["Contract.docx"]);
    let contract = Stetdoc::Writes(files.first());
    for command in [
        "stetdoc edit --ops b.txt Contract.docx --in-place",
        "stetdoc replace --author Jane Contract.docx a b --in-place",
        "stetdoc comment --range p1..p3 --clause 2 Contract.docx a b --in-place",
        "stetdoc edit --scope p1 --quotes keep --offsets utf16 --date D Contract.docx --in-place",
        "stetdoc edit Contract.docx --in-place --dry-run",
        "stetdoc edit Free.docx --output=Contract.docx",
        "stetdoc edit Free.docx -oContract.docx",
        "stetdoc edit Free.docx -o=Contract.docx",
        "stetdoc replace Free.docx a b -o Contract.docx --dry-run",
        "stetdoc unpack Free.docx --output Contract.docx",
    ] {
        assert_eq!(stetdoc(&words(command), &files, &[]), contract, "{command}");
    }
    for command in [
        "stetdoc edit Contract.docx --inplace",
        "stetdoc edit Contract.docx --in",
        "stetdoc edit Contract.docx -i",
        "stetdoc replace Contract.docx a b --find x",
        "stetdoc edit Contract.docx --ops --in-place",
        "stetdoc frobnicate Contract.docx",
    ] {
        assert_eq!(
            stetdoc(&words(command), &files, &[]),
            Stetdoc::Unknown,
            "{command}"
        );
    }
    for command in [
        "stetdoc edit Contract.docx --ops ops.txt",
        "stetdoc edit Contract.docx --ops ops.txt -o Out.docx",
        "stetdoc replace Contract.docx --author Jane a b --dry-run",
        "stetdoc edit Free.docx --in-place --ops Contract.txt",
        "stetdoc compare Contract.docx Free.docx -o Out.docx",
        "stetdoc view Contract.docx --anything",
        "stetdoc -- edit",
    ] {
        assert_eq!(
            stetdoc(&words(command), &files, &[]),
            Stetdoc::Fine,
            "{command}"
        );
    }
}

/// A word the path layer found held (a symlink's name) counts as held too.
#[test]
fn a_path_layer_hit_counts() {
    let hits = ["Link.docx".to_owned()];
    assert_eq!(
        stetdoc(&words("stetdoc edit a.docx -o Link.docx"), &[], &hits),
        Stetdoc::Writes(None)
    );
    assert_eq!(
        stetdoc(&words("stetdoc edit a.docx -o b.docx"), &[], &hits),
        Stetdoc::Fine
    );
}

#[test]
fn options_parse_as_clap_reads_them() {
    let parsed = parse(
        &["--ops", "b.txt", "Doc.docx", "--", "--in-place", "-o"],
        &["ops", "output"],
        &["in-place"],
    );
    assert_eq!(parsed.positionals, ["Doc.docx", "--in-place", "-o"]);
    assert!(!parsed.in_place && !parsed.unknown && parsed.outputs.is_empty());
    let parsed = parse(&["-o", "-", "x"], &["output"], &[]);
    assert_eq!(parsed.outputs, ["-"]);
    assert_eq!(parsed.positionals, ["x"]);
}
