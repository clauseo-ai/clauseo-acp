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
        "tree -p clauseo-protocol -e normal,build --all-features --target all --prefix depth --format {p} [{f}] --locked --offline"
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
    let out = "0clauseo-protocol v0.1.0 (/ws/crates/protocol) []\n\
               1serde v1.0.229 [alloc,default,derive,serde_derive,std]\n\
               2serde_derive v1.0.229 (proc-macro) [default]\n\
               1pm v0.1.0 (proc-macro) (/ws/vendor/pm (old) [x]) [a-b,c_d,e+f,g.h,i/j]\n\
               1gitdep v0.2.0-rc.1 (https://example.com/r.git#0123abcd) []\n\
               1win v0.1.0 (C:\\ws\\vendor\\win) []\n";
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
    let out = "0clauseo-protocol v0.1.0 (/ws) []\n\
               1left v0.1.0 (/ws/vendor/left) []\n\
               2inner v0.1.0 (/ws/vendor/inner) []\n\
               3tauri v0.1.0 (/ws/vendor/tauri) []\n\
               1right v0.1.0 (/ws/vendor/right) []\n\
               2inner v0.1.0 (/ws/vendor/inner) [] (*)\n\
               2tauri v0.1.0 (/ws/vendor/tauri) []\n";
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
    let out = "0clauseo-protocol v0.1.0 (/ws) []\n\
               1x v0.1.0 []\n\
               2foo v0.1.0 []\n\
               1y v0.1.0 []\n\
               2foo v0.1.0 [f]\n\
               3b v0.1.0 []\n";
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
    let out = "0clauseo-protocol v0.1.0 (/ws) []\n\
               1cc v1.0.0 []\n\
               2jobserver v0.1.0 []\n\
               1mid v0.1.0 []\n\
               2cc v1.0.0 []\n\
               3jobserver v0.1.0 []\n";
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

#[test]
fn a_line_of_another_shape_or_out_of_order_is_an_error() {
    let root = "0clauseo-protocol v0.1.0 (/ws) []\n";
    let parse = |rest: &str| Tree::parse(&format!("{root}{rest}"), "clauseo-protocol");
    let shape = |n: usize, text: &str| {
        Err(format!(
            "line {n} `{text}` is not `<depth><name> v<version>` followed by `(proc-macro)`, `(<source>)`, `[<features>]` and `(*)`"
        ))
    };
    let order = |n: usize, text: &str, deepest: usize| {
        Err(format!(
            "line {n} `{text}` breaks the tree: the first line is `clauseo-protocol` at depth 0, and each later line is one to {deepest} deep"
        ))
    };
    for bad in [
        "├── serde v1.0.0 []",
        "1serde 1.0.0 []",
        "1serde v1.0.0",
        "1serde v1.0.0 /ws []",
        "1serde v1.0.0 () []",
        "1serde v1.0.0 [a b]",
        "1serde v1.0.0 [] (*) (*)",
        "",
    ] {
        assert_eq!(parse(&format!("{bad}\n")), shape(2, bad));
    }
    assert_eq!(parse("3serde v1.0.0 []\n"), order(2, "3serde v1.0.0 []", 1));
    assert_eq!(parse("0other v1.0.0 []\n"), order(2, "0other v1.0.0 []", 1));
    assert_eq!(
        parse("1serde v1.0.0 [] (*)\n"),
        Err(
            "line 2 `1serde v1.0.0 [] (*)` marks a package not printed before with `(*)`"
                .to_owned()
        )
    );
    let under_a_star = "1a v1.0.0 []\n2b v1.0.0 []\n1a v1.0.0 [] (*)\n2b v1.0.0 []\n";
    assert_eq!(parse(under_a_star), order(5, "2b v1.0.0 []", 1));
    assert_eq!(
        Tree::parse("0other v1.0.0 []\n", "clauseo-protocol"),
        order(1, "0other v1.0.0 []", 0)
    );
    assert_eq!(
        Tree::parse("0clauseo-protocol v0.1.0 [] (*)\n", "clauseo-protocol"),
        order(1, "0clauseo-protocol v0.1.0 [] (*)", 0)
    );
    assert_eq!(
        Tree::parse("", "clauseo-protocol"),
        Err("it printed no line for `clauseo-protocol`".to_owned())
    );
}
