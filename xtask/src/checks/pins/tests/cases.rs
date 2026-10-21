//! The scratch repos of the pins check's tests and planted cases. Every version here is made up:
//! the homes in each repo pin Rust 3.4.5, pnpm 7.8.9 and Node 6.7.0, and `TOOLS` and `BINARIES`
//! below pin cargo-fake 5.6.7 and lefthook 9.8.7, so no test repeats a real pin. The planted
//! harness runs the check with the real `TOOLS` and `BINARIES`, so a planted case never names
//! one of their versions; the cases that do are in `fake_pins.rs`.

use super::super::inspect;
use crate::checks::Inspection;
use crate::checks::planted::Repo;
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
        Scratch::create_dir_all(parent).expect("create the folders");
    }
    Scratch::write(path, text).expect("write the file");
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

/// A repo with `rel` written as `text`.
fn with_file(case: &str, rel: &str, text: &str) -> Repo {
    let dir = repo(case);
    put(&dir, rel, text);
    Repo::new(dir)
}

/// A repo whose `.github/workflows/ci.yml` is `text`.
fn workflow(case: &str, text: &str) -> Repo {
    with_file(case, ".github/workflows/ci.yml", text)
}

/// A repo whose `lefthook.yml` is `text`.
fn lefthook(case: &str, text: &str) -> Repo {
    with_file(case, "lefthook.yml", text)
}

// The Rust version.

pub(super) fn rust_version_matches() -> Repo {
    Repo::new(repo("rust-version-matches"))
}

pub(super) fn rust_version_differs() -> Repo {
    with_file(
        "rust-version-differs",
        "Cargo.toml",
        "[workspace.package]\nedition = \"2024\"\nrust-version = \"3.3\"\n",
    )
}

pub(super) fn rust_version_in_a_dotted_key() -> Repo {
    with_file(
        "rust-version-dotted",
        "Cargo.toml",
        "[workspace]\nmembers = []\npackage.rust-version = \"3.5.0\"\n",
    )
}

pub(super) fn rust_version_in_a_member() -> Repo {
    let dir = repo("rust-version-member");
    put(
        &dir,
        "crates/desktop/Cargo.toml",
        "[package]\nname = \"clauseo-desktop\"\nrust-version = \"2.4\"\n",
    );
    Repo::desktop(dir)
}

pub(super) fn rust_version_inherited_by_a_member() -> Repo {
    let dir = repo("rust-version-inherited");
    put(
        &dir,
        "crates/desktop/Cargo.toml",
        "[package]\nname = \"clauseo-desktop\"\nrust-version.workspace = true\n",
    );
    Repo::desktop(dir)
}

pub(super) fn manifest_not_toml() -> Repo {
    with_file("manifest-not-toml", "Cargo.toml", "[workspace\n")
}

pub(super) fn toolchain_missing() -> Repo {
    let dir = repo("toolchain-missing");
    Scratch::remove_file(dir.join("rust-toolchain.toml")).expect("remove the toolchain file");
    Repo::new(dir)
}

pub(super) fn channel_not_a_version() -> Repo {
    with_file(
        "channel-not-a-version",
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"stable\"\n",
    )
}

pub(super) fn package_manager_not_pnpm() -> Repo {
    with_file(
        "package-manager-not-pnpm",
        "ui/package.json",
        "{ \"engines\": { \"node\": \">=6.7.0\" }, \"packageManager\": \"yarn@1.2.3\" }",
    )
}

pub(super) fn engines_node_missing() -> Repo {
    with_file(
        "engines-node-missing",
        "ui/package.json",
        "{ \"packageManager\": \"pnpm@7.8.9+sha512.abc\" }",
    )
}

// The lefthook comment.

/// The comment names the lefthook version the real `BINARIES` pins, read from it, since the
/// check runs as `cargo xtask check` does, with the real pins.
pub(super) fn lefthook_comment_matches() -> Repo {
    let pinned = crate::setup::BINARIES
        .iter()
        .find(|binary| binary.name == "lefthook")
        .expect("BINARIES pins lefthook");
    let text = format!(
        "# lefthook {}: `root` runs the command there.\npre-commit:\n  parallel: true\n",
        pinned.version
    );
    lefthook("lefthook-matches", &text)
}

pub(super) fn lefthook_comment_differs() -> Repo {
    lefthook(
        "lefthook-differs",
        "pre-commit:\n  commands:\n    # lefthook 9.8.6: `root` runs the command there.\n    \
         fmt: { run: cargo fmt }\n",
    )
}

pub(super) fn lefthook_comment_after_a_value() -> Repo {
    lefthook(
        "lefthook-trailing",
        "pre-commit:\n  parallel: true # as of Lefthook v10.0:\n  commands:\n    \
         fmt: { run: \"cargo fmt # x\" } # lefthook 9.0.0\n",
    )
}

pub(super) fn lefthook_text_in_a_string() -> Repo {
    lefthook(
        "lefthook-string",
        "pre-commit:\n  commands:\n    a: { run: \"echo '# lefthook 1.0.0'\" }\n    b:\n      \
         run: |\n        # lefthook 1.0.0 in a block scalar\n        true\n",
    )
}

pub(super) fn lefthook_not_yaml() -> Repo {
    lefthook("lefthook-not-yaml", "pre-commit:\n  commands: [\n")
}

// Workflows.

pub(super) fn workflow_reads_the_homes() -> Repo {
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

pub(super) fn pnpm_version_input() -> Repo {
    workflow(
        "pnpm-input",
        "jobs:\n  a:\n    steps:\n      - uses: pnpm/action-setup@v4\n        with:\n\
         \x20         version: 10\n",
    )
}

pub(super) fn node_version_input() -> Repo {
    workflow(
        "node-input",
        "jobs:\n  a:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n\
         \x20         node-version: lts/*\n",
    )
}

pub(super) fn toolchain_inputs() -> Repo {
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

pub(super) fn toolchain_in_the_ref() -> Repo {
    workflow(
        "toolchain-ref",
        "jobs:\n  a:\n    steps:\n      - uses: dtolnay/rust-toolchain@1.80.0\n",
    )
}

pub(super) fn toolchain_in_a_commit_comment() -> Repo {
    workflow(
        "toolchain-commit",
        "jobs:\n  a:\n    steps:\n\
         \x20     - uses: dtolnay/rust-toolchain@0123456789abcdef0123456789abcdef01234567 # 1.80.0\n",
    )
}

pub(super) fn quoted_keys_and_flow_mappings() -> Repo {
    workflow(
        "quoted-flow",
        "jobs:\n  a:\n    steps:\n\
         \x20     - \"uses\": 'pnpm/action-setup@v4'\n        \"with\": { \"version\": \"10\" }\n\
         \x20     - { uses: Actions/Setup-Node@v4, with: { Node-Version: 20 } }\n",
    )
}

pub(super) fn anchors_aliases_and_merge_keys() -> Repo {
    workflow(
        "anchors",
        "x-pnpm: &pnpm\n  version: 10\nx-node: &node { node-version: 20 }\njobs:\n  a:\n    steps:\n\
         \x20     - uses: pnpm/action-setup@v4\n        with: *pnpm\n\
         \x20     - uses: pnpm/action-setup@v4\n        with: *pnpm\n\
         \x20     - uses: actions/setup-node@v4\n        with:\n          <<: *node\n",
    )
}

pub(super) fn a_yaml_extension() -> Repo {
    with_file(
        "yaml-extension",
        ".github/workflows/release.yaml",
        "jobs:\n  a:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n\
         \x20         node-version: 20\n",
    )
}

pub(super) fn versions_in_run_steps() -> Repo {
    workflow(
        "run-steps",
        "jobs:\n  a:\n    steps:\n      - run: npm i -g pnpm@7.8.9\n\
         \x20     - run: |\n          corepack enable\n          corepack prepare pnpm@7.8 --activate\n\
         \x20     - run: rustup toolchain install 3.4.5\n\
         \x20     - run: cargo install cargo-fake@5.6.7 lefthook@9.8.7\n",
    )
}

pub(super) fn versions_in_other_values() -> Repo {
    workflow(
        "other-values",
        "env:\n  NODE: \"6.7\"\njobs:\n  a:\n    container: rust:3.4\n    steps:\n\
         \x20     - uses: docker://rust:3.4.5\n\
         \x20     - uses: taiki-e/install-action@v2\n        with:\n          tool: cargo-fake@5.6.7\n",
    )
}

pub(super) fn pnpm_copy_in_a_run_step() -> Repo {
    workflow(
        "pnpm-copy-run",
        "jobs:\n  a:\n    steps:\n      - run: npm i -g pnpm@7.8.9\n",
    )
}

pub(super) fn node_major_minor_in_a_block_scalar() -> Repo {
    workflow(
        "node-block-scalar",
        "jobs:\n  a:\n    steps:\n      - run: |\n          echo start\n          \
         fnm install 6.7 --corepack-enabled\n",
    )
}

pub(super) fn rust_copy_in_a_flow_mapping() -> Repo {
    workflow(
        "rust-flow-mapping",
        "jobs:\n  a:\n    env: { RUSTUP_TOOLCHAIN: \"3.4.5\" }\n    steps:\n      - run: true\n",
    )
}

pub(super) fn copy_under_a_quoted_key() -> Repo {
    workflow(
        "copy-quoted-key",
        "\"env\":\n  'Rustup_Toolchain': \"3.3\"\njobs:\n  a:\n    steps:\n      - run: true\n",
    )
}

pub(super) fn copy_through_an_alias() -> Repo {
    workflow(
        "copy-alias",
        "x-rust: &rust \"rustup toolchain install 3.4.5\"\njobs:\n  a:\n    steps:\n\
         \x20     - run: *rust\n  b:\n    steps:\n      - run: *rust\n",
    )
}

pub(super) fn copy_in_a_flow_sequence() -> Repo {
    workflow(
        "copy-flow-sequence",
        "jobs:\n  a:\n    strategy:\n      matrix:\n        image: [node:6.7.0]\n",
    )
}

pub(super) fn an_upper_case_yml_extension() -> Repo {
    with_file(
        "upper-case-yml",
        ".github/workflows/CI.YML",
        "jobs:\n  a:\n    steps:\n      - run: rustup toolchain install 3.4.5\n",
    )
}

/// GitHub reads only the top level of `.github/workflows/`, and so does the check.
pub(super) fn a_workflow_in_a_subfolder_passes() -> Repo {
    with_file(
        "workflow-subfolder",
        ".github/workflows/old/ci.yml",
        "jobs:\n  a:\n    steps:\n      - run: rustup toolchain install 3.4.5\n",
    )
}

pub(super) fn rust_version_in_a_root_package() -> Repo {
    with_file(
        "rust-version-root-package",
        "Cargo.toml",
        "[package]\nname = \"root\"\nrust-version = \"3.3\"\n",
    )
}

pub(super) fn rust_version_in_a_quoted_member_key() -> Repo {
    let dir = repo("rust-version-quoted-member");
    put(
        &dir,
        "crates/desktop/Cargo.toml",
        "[\"package\"]\nname = \"clauseo-desktop\"\n\"rust-version\" = \"2.4\"\n",
    );
    Repo::desktop(dir)
}

pub(super) fn rust_version_in_an_inline_member_table() -> Repo {
    let dir = repo("rust-version-inline-member");
    put(
        &dir,
        "crates/desktop/Cargo.toml",
        "package = { name = \"clauseo-desktop\", rust-version = \"2.4\" }\n",
    );
    Repo::desktop(dir)
}

pub(super) fn lefthook_comment_at_form() -> Repo {
    lefthook(
        "lefthook-at-form",
        "pre-commit:\n  commands: [fmt] #see LEFTHOOK@9.8.6\n",
    )
}

pub(super) fn workflow_not_yaml() -> Repo {
    workflow("workflow-not-yaml", "jobs:\n  a: {\n")
}

// The forms that name a tool, and the places they are read.

/// Composite actions under `.github/actions/`, as `action.yml` and `action.yaml`, nested too.
pub(super) fn pnpm_and_node_in_a_composite_action() -> Repo {
    let dir = repo("composite-action");
    put(
        &dir,
        ".github/actions/setup/action.yml",
        "name: setup\nruns:\n  using: composite\n  steps:\n\
         \x20   - uses: pnpm/action-setup@v4\n      with:\n        version: 7\n\
         \x20   - run: npm i -g pnpm@7.8.9\n      shell: bash\n",
    );
    put(
        &dir,
        ".github/actions/tools/node/action.yaml",
        "runs:\n  using: composite\n  steps:\n    - uses: actions/setup-node@v4\n\
         \x20     with: { node-version: 6 }\n",
    );
    put(
        &dir,
        ".github/actions/setup/notes.yml",
        "a: npm i -g pnpm@7\n",
    );
    Repo::new(dir)
}

pub(super) fn drifted_pnpm_and_node_container() -> Repo {
    workflow(
        "drift-npm-container",
        "jobs:\n  a:\n    container: node:6\n    services:\n      db:\n\
         \x20       image: docker.io/library/node:6.7-alpine\n    steps:\n\
         \x20     - run: npm i -g pnpm@7\n",
    )
}

/// The cargo tool is a real one in `TOOLS`, at a version it does not pin: the message names
/// the tool and the version written, never the pinned one.
pub(super) fn corepack_and_cargo_install_versions() -> Repo {
    workflow(
        "corepack-cargo-install",
        "jobs:\n  a:\n    steps:\n      - run: corepack prepare pnpm@7.8.8 --activate\n\
         \x20     - run: |\n          cargo install --locked cargo-deny \\\n            \
         --version 0.0.1\n\
         \x20     - uses: taiki-e/install-action@v2\n        with:\n\
         \x20         tool: cargo-nextest@0.0.1\n",
    )
}

pub(super) fn rustup_and_plus_toolchains() -> Repo {
    workflow(
        "rustup-forms",
        "jobs:\n  a:\n    steps:\n      - run: rustup default 3.3 && cargo +3.4.5 build\n\
         \x20     - run: rustup component add clippy --toolchain 3.4\n\
         \x20     - uses: docker://rust:3.4.5\n",
    )
}

/// A number that belongs to no pinned tool, even the Rust major.minor, and channels pass.
pub(super) fn numbers_that_name_no_tool() -> Repo {
    workflow(
        "numbers-no-tool",
        "env:\n  NODE: \"6.7\"\njobs:\n  a:\n    steps:\n\
         \x20     - uses: actions/setup-python@v5\n        with:\n          python-version: '3.4'\n\
         \x20     - run: echo 3.4.5 7.8.9 && rustup toolchain install nightly\n\
         \x20     - run: cargo +nightly fuzz run a && npx pnpm@latest i\n",
    )
}

/// The toolchain a commit of dtolnay/rust-toolchain installs cannot be told without a comment.
pub(super) fn dtolnay_at_a_bare_commit() -> Repo {
    workflow(
        "dtolnay-bare-commit",
        "jobs:\n  a:\n    steps:\n\
         \x20     - uses: dtolnay/rust-toolchain@0123456789abcdef0123456789abcdef01234567\n",
    )
}

pub(super) fn engines_node_with_a_caret() -> Repo {
    with_file(
        "engines-node-caret",
        "ui/package.json",
        "{ \"engines\": { \"node\": \"^6.7.0\" }, \"packageManager\": \"pnpm@7.8.9\" }",
    )
}
