//! The scratch repos of the pins check's tests and planted cases. Every version here is made up:
//! the homes in each repo pin Rust 3.4.5, pnpm 7.8.9 and Node 6.7.0, and `TOOLS` and `BINARIES`
//! below pin cargo-fake 5.6.7 and lefthook 9.8.7, so no test repeats a real pin.

use super::super::inspect;
use crate::checks::Inspection;
use crate::scratch::Scratch;
use crate::setup::{Binary, Packing, Tool};
use crate::workspace::Context;
use std::path::Path;

/// The cargo tools the cases pin.
pub(super) const TOOLS: &[Tool] = &[Tool {
    name: "cargo-fake",
    version: "5.6.7",
}];

/// The binaries the cases pin.
pub(super) const BINARIES: &[Binary] = &[Binary {
    name: "lefthook",
    version: "9.8.7",
    sha256: &[],
    repo: "evilmartians/lefthook",
    winget: "evilmartians.lefthook",
    asset: |_, _, _| None,
    packing: Packing::Bare,
}];

/// The homes of a repo whose pins all read.
const TOOLCHAIN: &str = "[toolchain]\nchannel = \"3.4.5\"\n";
const PACKAGE_JSON: &str =
    "{ \"engines\": { \"node\": \">=6.7.0\" }, \"packageManager\": \"pnpm@7.8.9\" }\n";
/// A workspace manifest whose `rust-version` matches the toolchain.
const CARGO_TOML: &str = "[workspace.package]\nrust-version = \"3.4\"\n";

/// Writes `text` to `dir/rel`, making its folders.
pub(super) fn put(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create the folders");
    }
    std::fs::write(path, text).expect("write the file");
}

/// A scratch repo with the three homes and a matching `Cargo.toml`.
pub(super) fn repo(case: &str) -> Scratch {
    let dir = Scratch::new(&format!("pins-{case}"));
    put(&dir, "rust-toolchain.toml", TOOLCHAIN);
    put(&dir, "ui/package.json", PACKAGE_JSON);
    put(&dir, "Cargo.toml", CARGO_TOML);
    dir
}

/// Runs the check on `dir` with no workspace members.
pub(super) fn check(dir: &Path) -> Inspection {
    check_in(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    })
}

/// Runs the check in `ctx` with the cases' `TOOLS` and `BINARIES`.
pub(super) fn check_in(ctx: &Context) -> Inspection {
    inspect(ctx, TOOLS, BINARIES)
}

/// The check on a repo with `rel` written as `text`.
fn with_file(case: &str, rel: &str, text: &str) -> Inspection {
    let dir = repo(case);
    put(&dir, rel, text);
    check(&dir)
}

/// The check on a repo whose `.github/workflows/ci.yml` is `text`.
fn workflow(case: &str, text: &str) -> Inspection {
    with_file(case, ".github/workflows/ci.yml", text)
}

/// The check on a repo whose `lefthook.yml` is `text`.
fn lefthook(case: &str, text: &str) -> Inspection {
    with_file(case, "lefthook.yml", text)
}

// The Rust version.

pub(super) fn rust_version_matches() -> Inspection {
    check(&repo("rust-version-matches"))
}

pub(super) fn rust_version_differs() -> Inspection {
    with_file(
        "rust-version-differs",
        "Cargo.toml",
        "[workspace.package]\nedition = \"2024\"\nrust-version = \"3.3\"\n",
    )
}

pub(super) fn rust_version_in_a_dotted_key() -> Inspection {
    with_file(
        "rust-version-dotted",
        "Cargo.toml",
        "[workspace]\nmembers = []\npackage.rust-version = \"3.5.0\"\n",
    )
}

pub(super) fn rust_version_in_a_member() -> Inspection {
    let dir = repo("rust-version-member");
    put(
        &dir,
        "crates/desktop/Cargo.toml",
        "[package]\nname = \"clauseo-desktop\"\nrust-version = \"2.4\"\n",
    );
    check_in(&crate::checks::tests::desktop_context(&dir))
}

pub(super) fn rust_version_inherited_by_a_member() -> Inspection {
    let dir = repo("rust-version-inherited");
    put(
        &dir,
        "crates/desktop/Cargo.toml",
        "[package]\nname = \"clauseo-desktop\"\nrust-version.workspace = true\n",
    );
    check_in(&crate::checks::tests::desktop_context(&dir))
}

pub(super) fn manifest_not_toml() -> Inspection {
    with_file("manifest-not-toml", "Cargo.toml", "[workspace\n")
}

pub(super) fn toolchain_missing() -> Inspection {
    let dir = repo("toolchain-missing");
    std::fs::remove_file(dir.join("rust-toolchain.toml")).expect("remove the toolchain file");
    check(&dir)
}

pub(super) fn channel_not_a_version() -> Inspection {
    with_file(
        "channel-not-a-version",
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"stable\"\n",
    )
}

pub(super) fn package_manager_not_pnpm() -> Inspection {
    with_file(
        "package-manager-not-pnpm",
        "ui/package.json",
        "{ \"engines\": { \"node\": \">=6.7.0\" }, \"packageManager\": \"yarn@1.2.3\" }",
    )
}

pub(super) fn engines_node_missing() -> Inspection {
    with_file(
        "engines-node-missing",
        "ui/package.json",
        "{ \"packageManager\": \"pnpm@7.8.9+sha512.abc\" }",
    )
}

// The lefthook comment.

pub(super) fn lefthook_comment_matches() -> Inspection {
    lefthook(
        "lefthook-matches",
        "# lefthook 9.8.7: `root` runs the command there.\npre-commit:\n  parallel: true\n",
    )
}

pub(super) fn lefthook_comment_differs() -> Inspection {
    lefthook(
        "lefthook-differs",
        "pre-commit:\n  commands:\n    # lefthook 9.8.6: `root` runs the command there.\n    \
         fmt: { run: cargo fmt }\n",
    )
}

pub(super) fn lefthook_comment_after_a_value() -> Inspection {
    lefthook(
        "lefthook-trailing",
        "pre-commit:\n  parallel: true # as of Lefthook v10.0:\n  commands:\n    \
         fmt: { run: \"cargo fmt # x\" } # lefthook 9.0.0\n",
    )
}

pub(super) fn lefthook_text_in_a_string() -> Inspection {
    lefthook(
        "lefthook-string",
        "pre-commit:\n  commands:\n    a: { run: \"echo '# lefthook 1.0.0'\" }\n    b:\n      \
         run: |\n        # lefthook 1.0.0 in a block scalar\n        true\n",
    )
}

pub(super) fn lefthook_not_yaml() -> Inspection {
    lefthook("lefthook-not-yaml", "pre-commit:\n  commands: [\n")
}

// Workflows.

pub(super) fn workflow_reads_the_homes() -> Inspection {
    workflow(
        "workflow-clean",
        "jobs:\n  build:\n    steps:\n\
         \x20     - uses: pnpm/action-setup@0123456789abcdef0123456789abcdef01234567 # v4.1.0\n\
         \x20       with:\n          package_json_file: ui/package.json\n\
         \x20     - uses: actions/setup-node@v4\n        with:\n\
         \x20         node-version-file: ui/package.json\n\
         \x20     - uses: dtolnay/rust-toolchain@stable\n        with:\n          components: clippy\n\
         \x20     - uses: dtolnay/rust-toolchain@0123456789abcdef0123456789abcdef01234567 # stable\n\
         \x20     - run: rustup toolchain install\n\
         \x20     - run: echo 13.4.56 v7.8.90 6.7.0.1 5.6.78\n",
    )
}

pub(super) fn pnpm_version_input() -> Inspection {
    workflow(
        "pnpm-input",
        "jobs:\n  a:\n    steps:\n      - uses: pnpm/action-setup@v4\n        with:\n\
         \x20         version: 10\n",
    )
}

pub(super) fn node_version_input() -> Inspection {
    workflow(
        "node-input",
        "jobs:\n  a:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n\
         \x20         node-version: lts/*\n",
    )
}

pub(super) fn toolchain_inputs() -> Inspection {
    workflow(
        "toolchain-inputs",
        "jobs:\n  a:\n    steps:\n\
         \x20     - uses: dtolnay/rust-toolchain@master\n        with:\n          toolchain: nightly\n\
         \x20     - uses: actions-rust-lang/setup-rust-toolchain@v1\n        with:\n\
         \x20         toolchain: stable\n\
         \x20     - uses: actions-rs/toolchain@v1\n        with:\n          toolchain: beta\n\
         \x20     - uses: moonrepo/setup-rust@v1\n        with:\n          channel: stable\n",
    )
}

pub(super) fn toolchain_in_the_ref() -> Inspection {
    workflow(
        "toolchain-ref",
        "jobs:\n  a:\n    steps:\n      - uses: dtolnay/rust-toolchain@1.80.0\n",
    )
}

pub(super) fn toolchain_in_a_commit_comment() -> Inspection {
    workflow(
        "toolchain-commit",
        "jobs:\n  a:\n    steps:\n\
         \x20     - uses: dtolnay/rust-toolchain@0123456789abcdef0123456789abcdef01234567 # 1.80.0\n",
    )
}

pub(super) fn quoted_keys_and_flow_mappings() -> Inspection {
    workflow(
        "quoted-flow",
        "jobs:\n  a:\n    steps:\n\
         \x20     - \"uses\": 'pnpm/action-setup@v4'\n        \"with\": { \"version\": \"10\" }\n\
         \x20     - { uses: Actions/Setup-Node@v4, with: { Node-Version: 20 } }\n",
    )
}

pub(super) fn anchors_aliases_and_merge_keys() -> Inspection {
    workflow(
        "anchors",
        "x-pnpm: &pnpm\n  version: 10\nx-node: &node { node-version: 20 }\njobs:\n  a:\n    steps:\n\
         \x20     - uses: pnpm/action-setup@v4\n        with: *pnpm\n\
         \x20     - uses: pnpm/action-setup@v4\n        with: *pnpm\n\
         \x20     - uses: actions/setup-node@v4\n        with:\n          <<: *node\n",
    )
}

pub(super) fn a_yaml_extension() -> Inspection {
    with_file(
        "yaml-extension",
        ".github/workflows/release.yaml",
        "jobs:\n  a:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n\
         \x20         node-version: 20\n",
    )
}

pub(super) fn versions_in_run_steps() -> Inspection {
    workflow(
        "run-steps",
        "jobs:\n  a:\n    steps:\n      - run: npm i -g pnpm@7.8.9\n\
         \x20     - run: |\n          corepack enable\n          corepack prepare pnpm@7.8 --activate\n\
         \x20     - run: rustup toolchain install 3.4.5\n\
         \x20     - run: cargo install cargo-fake@5.6.7 lefthook@9.8.7\n",
    )
}

pub(super) fn versions_in_other_values() -> Inspection {
    workflow(
        "other-values",
        "env:\n  NODE: \"6.7\"\njobs:\n  a:\n    container: rust:3.4\n    steps:\n\
         \x20     - uses: docker://rust:3.4.5\n\
         \x20     - uses: taiki-e/install-action@v2\n        with:\n          tool: cargo-fake@5.6.7\n",
    )
}

pub(super) fn workflow_not_yaml() -> Inspection {
    workflow("workflow-not-yaml", "jobs:\n  a: {\n")
}
