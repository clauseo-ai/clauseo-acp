//! Tests of the cargo runs and the `cargo tree` reading shared by `dependencies` and `plugins`.
//! Both checks also run real `cargo tree --offline` on scratch workspaces in their own tests.

use super::*;

#[test]
fn the_fix_of_a_failed_run_fits_its_cause() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "-p", "x"]);
    let lock = "error: the lock file /ws/Cargo.lock needs to be updated but --locked was passed to prevent this";
    assert!(failure_fix(&cmd, lock).contains("cargo update --workspace"));
    let offline = "error: no matching package named `serde` found\nAs a reminder, you're using offline mode (--offline)";
    assert!(failure_fix(&cmd, offline).contains("cargo fetch --locked"));
    assert_eq!(
        failure_fix(&cmd, "error: bad manifest"),
        "fix what cargo reports above until `cargo tree -p x` succeeds: this check reads the dependency graph from it"
    );
}

#[test]
fn the_tree_run_resolves_the_crate_alone_with_all_its_features_for_every_target() {
    assert_eq!(
        tree_args("clauseo-protocol", &["--locked", "--offline"]).join(" "),
        "tree -p clauseo-protocol -e normal,build --all-features --target all --prefix indent --charset ascii --format {p} [{f}] --locked --offline"
    );
}

/// Each node of `tree` as `name version: dep, dep`, in the tree's order.
fn listed(tree: &Tree) -> Vec<String> {
    (0..tree.packages.len())
        .map(|id| {
            let deps: Vec<&str> = tree.deps(id).iter().map(|&dep| tree.name(dep)).collect();
            format!(
                "{} {}: {}",
                tree.name(id),
                tree.version(id),
                deps.join(", ")
            )
        })
        .collect()
}

#[test]
fn every_line_shape_cargo_prints_parses() {
    let out = "clauseo-protocol v0.1.0 (/ws/crates/protocol) []\n\
               |-- serde v1.0.229 [alloc,default,derive,serde_derive,std]\n\
               |   `-- serde_derive v1.0.229 (proc-macro) [default]\n\
               |-- pm v0.1.0 (proc-macro) (/ws/vendor/pm (old) [x]) [a-b,c_d,e+f,g.h,i/j]\n\
               |-- gitdep v0.2.0-rc.1 (https://example.com/r.git#0123abcd) []\n\
               `-- win v0.1.0 (C:\\ws\\vendor\\win) []\n";
    let tree = Tree::parse(out, "clauseo-protocol").expect("it parses");
    assert_eq!(
        listed(&tree),
        [
            "clauseo-protocol 0.1.0: serde, pm, gitdep, win",
            "serde 1.0.229: serde_derive",
            "serde_derive 1.0.229: ",
            "pm 0.1.0: ",
            "gitdep 0.2.0-rc.1: ",
            "win 0.1.0: ",
        ]
    );
}

#[test]
fn a_starred_line_is_the_node_printed_before_with_everything_under_it() {
    let out = r"clauseo-protocol v0.1.0 (/ws) []
|-- left v0.1.0 (/ws/vendor/left) []
|   `-- inner v0.1.0 (/ws/vendor/inner) []
|       `-- tauri v0.1.0 (/ws/vendor/tauri) []
`-- right v0.1.0 (/ws/vendor/right) []
    |-- inner v0.1.0 (/ws/vendor/inner) [] (*)
    `-- tauri v0.1.0 (/ws/vendor/tauri) []
";
    let tree = Tree::parse(out, "clauseo-protocol").expect("it parses");
    assert_eq!(
        listed(&tree),
        [
            "clauseo-protocol 0.1.0: left, right",
            "left 0.1.0: inner",
            "inner 0.1.0: tauri",
            "tauri 0.1.0: ",
            "right 0.1.0: inner, tauri",
        ]
    );
    let reach = tree.reach(4);
    let names = |ids: &[usize]| ids.iter().map(|&id| tree.name(id)).collect::<Vec<_>>();
    assert_eq!(names(reach.order()), ["right", "inner", "tauri"]);
    assert_eq!(names(&reach.path(3)), ["right", "tauri"]);
    assert_eq!(names(&tree.reach(1).path(3)), ["left", "inner", "tauri"]);
}

/// A package built for the host by a build dependency with other features than its target build
/// is two nodes: what one build pulls in is never under the other.
#[test]
fn a_package_built_twice_with_other_features_is_two_nodes() {
    let out = r"clauseo-protocol v0.1.0 (/ws) []
|-- x v0.1.0 []
|   `-- foo v0.1.0 []
`-- y v0.1.0 []
    `-- foo v0.1.0 [f]
        `-- b v0.1.0 []
";
    let tree = Tree::parse(out, "clauseo-protocol").expect("it parses");
    assert_eq!(
        listed(&tree),
        [
            "clauseo-protocol 0.1.0: x, y",
            "x 0.1.0: foo",
            "foo 0.1.0: ",
            "y 0.1.0: foo",
            "foo 0.1.0: b",
            "b 0.1.0: ",
        ]
    );
    assert_eq!(tree.reach(1).order(), [1, 2]);
}

#[test]
fn a_package_printed_in_full_twice_with_the_same_features_is_one_node() {
    let out = r"clauseo-protocol v0.1.0 (/ws) []
|-- cc v1.0.0 []
|   `-- jobserver v0.1.0 []
`-- mid v0.1.0 []
    `-- cc v1.0.0 []
        `-- jobserver v0.1.0 []
";
    let tree = Tree::parse(out, "clauseo-protocol").expect("it parses");
    assert_eq!(
        listed(&tree),
        [
            "clauseo-protocol 0.1.0: cc, mid",
            "cc 1.0.0: jobserver",
            "jobserver 0.1.0: ",
            "mid 0.1.0: cc",
        ]
    );
}

/// `s` is a normal edge of the crate and, from the second line of the build section, also a
/// build edge. Its own build section holds `t`, which is no direct edge.
#[test]
fn a_direct_dependency_records_the_sections_its_lines_came_under() {
    let out = r"clauseo-protocol v0.1.0 (/ws) []
|-- s v0.1.0 (/ws/vendor/s) [f1,f2]
|   |-- b1 v0.1.0 []
|   [build-dependencies]
|   `-- t v0.1.0 []
|-- q v0.1.0 []
|   `-- s v0.1.0 (/ws/vendor/s) [f1,f2] (*)
[build-dependencies]
|-- q v0.1.0 [] (*)
`-- s v0.1.0 (/ws/vendor/s) [f1,f2] (*)
";
    let tree = Tree::parse(out, "clauseo-protocol").expect("it parses");
    let id = |name: &str| (0..tree.packages.len()).find(|&id| tree.name(id) == name);
    let (s, q, t) = (id("s").unwrap(), id("q").unwrap(), id("t").unwrap());
    assert_eq!(tree.sections(s), [Section::Normal, Section::Build]);
    assert_eq!(tree.sections(q), [Section::Normal, Section::Build]);
    assert_eq!(tree.sections(t), []);
    assert_eq!(tree.sections(Tree::ROOT), []);
}

/// A header under a package that is not a direct dependency labels only the edges below that
/// package, so a direct dependency above it keeps the section of its own line.
#[test]
fn a_header_below_a_direct_dependency_leaves_its_section_normal() {
    let out = r"clauseo-protocol v0.1.0 (/ws) []
|-- s v0.1.0 []
|   [build-dependencies]
|   `-- t v0.1.0 []
`-- r v0.1.0 []
    `-- u v0.1.0 []
";
    let tree = Tree::parse(out, "clauseo-protocol").expect("it parses");
    let s = (0..tree.packages.len()).find(|&id| tree.name(id) == "s");
    assert_eq!(tree.sections(s.unwrap()), [Section::Normal]);
    assert_eq!(listed(&tree)[1], "s 0.1.0: t");
}

#[test]
fn a_line_of_another_shape_or_out_of_order_is_an_error() {
    let root = "clauseo-protocol v0.1.0 (/ws) []\n";
    let parse = |rest: &str| Tree::parse(&format!("{root}{rest}"), "clauseo-protocol");
    let shape = |n: usize, text: &str| {
        Err(format!(
            "line {n} `{text}` is not a node line (`<prefix><name> v<version>`, then `(proc-macro)`, `(<source>)`, `[<features>]` and `(*)`) or a `[build-dependencies]` header"
        ))
    };
    let order = |n: usize, text: &str, deepest: usize| {
        Err(format!(
            "line {n} `{text}` breaks the tree: the first line is `clauseo-protocol` at depth 0, and each later line is one to {deepest} deep"
        ))
    };
    for bad in [
        "├── serde v1.0.0 []",
        "serde 1.0.0 []",
        "|-- serde v1.0.0",
        "|-- serde v1.0.0 /ws []",
        "|-- serde v1.0.0 () []",
        "|-- serde v1.0.0 [a b]",
        "|-- serde v1.0.0 [] (*) (*)",
        "|  -- serde v1.0.0 []",
        "  |-- serde v1.0.0 []",
        "    serde v1.0.0 []",
        "|-- [dev-dependencies]",
        "[dev-dependencies]",
        "",
    ] {
        assert_eq!(parse(&format!("{bad}\n")), shape(2, bad));
    }
    assert_eq!(
        parse("|   |   |-- serde v1.0.0 []\n"),
        order(2, "|   |   |-- serde v1.0.0 []", 1)
    );
    assert_eq!(parse("other v1.0.0 []\n"), order(2, "other v1.0.0 []", 1));
    assert_eq!(
        parse("|-- serde v1.0.0 [] (*)\n"),
        Err(
            "line 2 `|-- serde v1.0.0 [] (*)` marks a package not printed before with `(*)`"
                .to_owned()
        )
    );
    let under_a_star =
        "|-- a v1.0.0 []\n|   `-- b v1.0.0 []\n|-- a v1.0.0 [] (*)\n|   `-- b v1.0.0 []\n";
    assert_eq!(parse(under_a_star), order(5, "|   `-- b v1.0.0 []", 1));
    let header_under_a_star = "|-- a v1.0.0 []\n|-- a v1.0.0 [] (*)\n|   [build-dependencies]\n";
    assert_eq!(
        parse(header_under_a_star),
        Err("line 4 `|   [build-dependencies]` is a section header with no package above it that can have edges".to_owned())
    );
    assert_eq!(
        Tree::parse("[build-dependencies]\n", "clauseo-protocol"),
        Err("line 1 `[build-dependencies]` is a section header with no package above it that can have edges".to_owned())
    );
    assert_eq!(
        Tree::parse("other v1.0.0 []\n", "clauseo-protocol"),
        order(1, "other v1.0.0 []", 0)
    );
    assert_eq!(
        Tree::parse("clauseo-protocol v0.1.0 [] (*)\n", "clauseo-protocol"),
        order(1, "clauseo-protocol v0.1.0 [] (*)", 0)
    );
    assert_eq!(
        Tree::parse("", "clauseo-protocol"),
        Err("it printed no line for `clauseo-protocol`".to_owned())
    );
}

#[test]
fn declared_says_each_line_brings_the_package_in_only_when_each_node_has_one_line() {
    let say = |nodes: &[Vec<u32>]| declared("`s`", "Cargo.toml", "b", nodes);
    assert_eq!(
        say(&[vec![7], vec![10]]),
        "`s` is declared at lines 7 and 10 of Cargo.toml, and each declaration brings b in"
    );
    assert_eq!(
        say(&[vec![7, 10]]),
        "`s` is declared at lines 7 and 10 of Cargo.toml. Cargo builds these declarations as one \
         package, and one or more of them brings b in"
    );
    assert_eq!(
        say(&[vec![7, 10], vec![13]]),
        "`s` is declared at lines 7, 10 and 13 of Cargo.toml. Cargo builds the declarations at \
         lines 7 and 10 as one package, and one or more of them brings b in. The declaration at \
         line 13 brings b in"
    );
}
