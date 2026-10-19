//! Tests of the `cargo tree` reading of the `dependencies` check: the line parser on its own,
//! and a package reached through two direct dependencies.

use super::super::graph::{TreeLine, parse_tree, tree_args};
use super::{PROTOCOL_ROW, manifest, run_on, tauri, vendor, workspace};
use crate::checks::Inspection;

/// Two direct dependencies that both bring tauri in, through the same `inner`: without
/// `--no-dedupe` the second path would print as `inner (*)` and lose tauri.
pub(in crate::checks::dependencies) fn two_paths_each_fail() -> Inspection {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\nleft = { path = \"../../vendor/left\" }\nright = { path = \"../../vendor/right\" }\n",
            ),
        ),
        vendor("left", "[dependencies]\ninner = { path = \"../inner\" }\n"),
        vendor("right", "[dependencies]\ninner = { path = \"../inner\" }\n"),
        vendor("inner", "[dependencies]\ntauri = { path = \"../tauri\" }\n"),
        tauri(),
    ];
    run_on(&workspace("two-paths", PROTOCOL_ROW, "", &crates))
}

fn line(depth: usize, name: &str) -> TreeLine {
    TreeLine {
        depth,
        name: name.to_owned(),
    }
}

#[test]
fn every_line_shape_cargo_prints_parses() {
    let out = "0clauseo-protocol v0.1.0 (/ws/crates/protocol)\n\
               1serde v1.0.229\n\
               2serde_derive v1.0.229 (proc-macro)\n\
               1pm v0.1.0 (proc-macro) (/ws/vendor/pm (old))\n\
               1gitdep v0.2.0-rc.1 (https://example.com/r.git#0123abcd)\n\
               1win v0.1.0 (C:\\ws\\vendor\\win)\n";
    assert_eq!(
        parse_tree(out, "clauseo-protocol"),
        Ok(vec![
            line(0, "clauseo-protocol"),
            line(1, "serde"),
            line(2, "serde_derive"),
            line(1, "pm"),
            line(1, "gitdep"),
            line(1, "win"),
        ])
    );
}

#[test]
fn a_line_of_another_shape_or_out_of_order_is_an_error() {
    let root = "0clauseo-protocol v0.1.0 (/ws)\n";
    let parse = |rest: &str| parse_tree(&format!("{root}{rest}"), "clauseo-protocol");
    let shape = |n: usize, text: &str| {
        Err(format!(
            "line {n} `{text}` is not `<depth><name> v<version>` followed by `(<source>)` or `(proc-macro)`"
        ))
    };
    assert_eq!(parse("1serde v1.0.0 (*)\n"), shape(2, "1serde v1.0.0 (*)"));
    assert_eq!(parse("├── serde v1.0.0\n"), shape(2, "├── serde v1.0.0"));
    assert_eq!(parse("1serde 1.0.0\n"), shape(2, "1serde 1.0.0"));
    assert_eq!(parse("1serde v1.0.0 /ws\n"), shape(2, "1serde v1.0.0 /ws"));
    assert_eq!(parse("1serde v1.0.0 ()\n"), shape(2, "1serde v1.0.0 ()"));
    assert_eq!(parse("\n"), shape(2, ""));
    assert_eq!(
        parse("3serde v1.0.0\n"),
        Err("line 2 `3serde v1.0.0` breaks the tree: the first line is `clauseo-protocol` at depth 0, and each later line is one to 1 deep".to_owned())
    );
    assert_eq!(
        parse("0other v1.0.0\n"),
        Err("line 2 `0other v1.0.0` breaks the tree: the first line is `clauseo-protocol` at depth 0, and each later line is one to 1 deep".to_owned())
    );
    assert_eq!(
        parse_tree("0other v1.0.0\n", "clauseo-protocol"),
        Err("line 1 `0other v1.0.0` breaks the tree: the first line is `clauseo-protocol` at depth 0, and each later line is one to 0 deep".to_owned())
    );
    assert_eq!(
        parse_tree("", "clauseo-protocol"),
        Err("it printed no line for `clauseo-protocol`".to_owned())
    );
}

#[test]
fn the_tree_run_resolves_the_crate_alone_with_all_its_features_for_every_target() {
    assert_eq!(
        tree_args("clauseo-protocol", &["--locked", "--offline"]).join(" "),
        "tree -p clauseo-protocol -e normal,build --all-features --target all --prefix depth --no-dedupe --format {p} --locked --offline"
    );
}
