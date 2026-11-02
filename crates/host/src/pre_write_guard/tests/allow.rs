//! Review round 3: the read-only exemption is narrow and whole-command. Each blocking finding's
//! probes are refused; reads a lawyer's agent makes, and writes to another file, still go.

use serde_json::json;

use super::backstop::{add, check};
use super::{Fixture, deny, guard_json};

/// Findings 1-6 and 8: a write hiding behind a reader, each refused.
#[test]
fn every_round_3_probe_is_refused() {
    let fx = Fixture::new("allow-probes");
    let contract = deny("Contract.docx");
    let let_through = [
        // 1: `sh -c`'s arguments, and its script under the whole-command rule.
        "sh -c 'cp x \"$1\"' _ Contract.docx",
        "bash -c 'tee \"$0\" </dev/null' Contract.docx",
        "bash -c 'cat \"$1\"' _ Contract.docx",
        // 2: `>&file` writes the file in bash.
        "cat x >&Contract.docx",
        "cat x >& Contract.docx",
        // 3: stetdoc's flag values are not its document.
        "stetdoc edit --ops b.txt Contract.docx --in-place",
        "stetdoc replace --author Jane Contract.docx a b --in-place",
        "stetdoc edit Free.docx --output=Contract.docx",
        "stetdoc edit Free.docx -oContract.docx",
        "stetdoc edit Contract.docx --inplace",
        // 4: only bare program names, no assignments, no functions, no `less`.
        "./cat Contract.docx",
        "/tmp/evil/cat Contract.docx",
        "cat() { cp x \"$1\"; }; cat Contract.docx",
        "function cat { cp x \"$1\"; }; cat Contract.docx",
        "PATH=/tmp/evil cat Contract.docx",
        "PATH=/tmp/evil; cat Contract.docx",
        "export PATH=/tmp/evil; cat Contract.docx",
        "LESSOPEN='|cp x %s' less Contract.docx",
        "export LESSOPEN='|cp x %s'; less Contract.docx",
        "less Contract.docx",
        // 5: every part of the command, not only the parts naming the file.
        "file Contract.docx | cut -d: -f1 | xargs truncate -s0",
        "wc -c Contract.docx | awk '{print $2}' | xargs cp x",
        "head -c0 Contract.docx; cp x \"$_\"",
        "cat <<< Contract.docx | xargs cp x",
        "cat Contract.docx; rm Free.docx",
        "cat Contract.docx | tee out.txt",
        "cat Contract.docx | sed -i s/a/b/ x",
        "cat Contract.docx | sort -o out.txt",
        "cat Contract.docx | uniq - out.txt",
        "cat Contract.docx | rg --pre ./x y",
        "cat Contract.docx | grep x Contract.docx",
        "cat <<EOF | sh\ncp x Contract.docx\nEOF",
        "env cat Contract.docx",
        "file -C -m Contract.docx",
        // 6: pandoc options that write or run code, in any spelling.
        "pandoc x.md --out Contract.docx",
        "pandoc x.md --out=Contract.docx",
        "pandoc x.md -so Contract.docx",
        "pandoc Contract.docx --lua-filter f.lua",
        "pandoc Contract.docx --filter ./f",
        "pandoc Contract.docx -d defaults.yaml",
        "pandoc Contract.docx --extract-media=media",
        // 8: quotes and escapes inside the name.
        "cp x Con\"\"tract.docx",
        "cp x Con\\tract.docx",
        "cd \"$(dirname \"$PWD\")\" && cp x m/Con\"\"tract.docx",
        "sh -c 'cp x Con\"\"tract.docx'",
        // Redirects: only a literal path naming no held file.
        "cat Contract.docx > Con*.docx",
        "cat Contract.docx > $out",
        "cat Contract.docx > ~/x",
        "cat Contract.docx > \"$HOME/x\"",
        "cat x &> Contract.docx",
    ]
    .into_iter()
    .filter(|command| check(&fx, "Bash", command) != contract)
    .collect::<Vec<_>>();
    assert!(let_through.is_empty(), "let through: {let_through:#?}");
    add(&fx, &["My Deal.docx", "~$ Deal.docx"]);
    assert_eq!(
        check(
            &fx,
            "Bash",
            "cd \"$(dirname \"$PWD\")\" && cp x m/My\\ Deal.docx"
        ),
        deny("My Deal.docx")
    );
}

/// The reads the exemption keeps, and writes that land on another file.
#[test]
fn reads_and_writes_elsewhere_still_go() {
    let fx = Fixture::new("allow-reads");
    for command in [
        "cat Contract.docx > copy.txt 2>&1",
        "cat Contract.docx >> notes.txt",
        "cat Contract.docx 2>/dev/null >&2",
        "cat Contract.docx 3>&1 1>&2 2>&3 3>&-",
        "cd sub && cat ../Contract.docx",
        "echo start; cat Contract.docx; pwd; ls; true",
        "command cat Contract.docx",
        "unzip -p Contract.docx word/document.xml | sed -n 1,5p | sort | uniq | cut -c1-80 \
         | tr a-z A-Z | nl | column -t | grep -v x | head",
        "cat Contract.docx | rg x",
        "textutil -convert txt -stdout Contract.docx | wc -w",
        "pandoc Contract.docx -t plain",
        "pandoc Contract.docx -o summary.md",
        "pandoc --output=out.txt Contract.docx",
        "pandoc -s Contract.docx -o -",
        "stetdoc view Contract.docx --outline",
        "stetdoc edit Contract.docx --ops ops.txt",
        "stetdoc replace Contract.docx --author Jane a b -o Out.docx",
        "sh -c 'cat Contract.docx | head'",
        "cat Contract.docx <<EOF\nnote\nEOF",
    ] {
        assert_eq!(check(&fx, "Bash", command), "", "{command}");
    }
}

/// A command given as an argument list is judged as one line, its parts quoted.
#[test]
fn an_argument_list_is_judged_as_one_line() {
    let fx = Fixture::new("allow-list");
    let run = |parts: &[&str]| guard_json(&fx.hook("Bash", &json!({ "command": parts })));
    assert_eq!(run(&["bash", "-lc", "cat Contract.docx | head"]), "");
    assert_eq!(run(&["cat", "it's", "Contract.docx"]), "");
    assert_eq!(
        run(&["bash", "-lc", "cp x \"$1\"", "_", "Contract.docx"]),
        deny("Contract.docx")
    );
    assert_eq!(run(&["cp", "x", "Contract.docx"]), deny("Contract.docx"));
}

/// Finding 7: a word the path layer resolves to a held file (here a symlink's name) is refused
/// unless a reader holds it.
#[cfg(unix)]
#[test]
fn a_path_layer_hit_is_refused_outside_a_reader() {
    let fx = Fixture::new("allow-link");
    crate::write::symlink(
        &fx.matter().join("Contract.docx"),
        &fx.matter().join("Link.docx"),
    )
    .unwrap();
    let contract = deny("Contract.docx");
    for command in [
        "cp x Link.docx",
        "cat x > Link.docx",
        "cat x | grep y Link.docx",
        "pandoc x.md -o Link.docx",
        "stetdoc edit Free.docx -o Link.docx",
        "bash -c 'cat \"$1\"' _ Link.docx",
    ] {
        assert_eq!(check(&fx, "Bash", command), contract, "{command}");
    }
    assert_eq!(check(&fx, "Bash", "cat Link.docx | head"), "");
}

/// Finding 7 on macOS: a name typed decomposed (NFD) names the composed (NFC) held file.
#[cfg(target_os = "macos")]
#[test]
fn a_held_name_in_another_unicode_form_is_refused() {
    let fx = Fixture::new("allow-nfd");
    add(&fx, &["P\u{e9}tition.docx", "~$tition.docx"]);
    assert_eq!(
        check(&fx, "Bash", "echo hi | tee Pe\u{301}tition.docx"),
        deny("P\u{e9}tition.docx")
    );
}
