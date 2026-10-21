//! The planted repos of the workflow files: the inputs and copies they hold, and parse errors.

use super::*;

pub(in crate::checks::pins::tests) fn workflow_reads_the_homes() -> Repo {
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

pub(in crate::checks::pins::tests) fn pnpm_version_input() -> Repo {
    workflow(
        "pnpm-input",
        "jobs:\n  a:\n    steps:\n      - uses: pnpm/action-setup@v4\n        with:\n\
         \x20         version: 10\n",
    )
}

pub(in crate::checks::pins::tests) fn node_version_input() -> Repo {
    workflow(
        "node-input",
        "jobs:\n  a:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n\
         \x20         node-version: lts/*\n",
    )
}

pub(in crate::checks::pins::tests) fn toolchain_inputs() -> Repo {
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

pub(in crate::checks::pins::tests) fn toolchain_in_the_ref() -> Repo {
    workflow(
        "toolchain-ref",
        "jobs:\n  a:\n    steps:\n      - uses: dtolnay/rust-toolchain@1.80.0\n",
    )
}

pub(in crate::checks::pins::tests) fn toolchain_in_a_commit_comment() -> Repo {
    workflow(
        "toolchain-commit",
        "jobs:\n  a:\n    steps:\n\
         \x20     - uses: dtolnay/rust-toolchain@0123456789abcdef0123456789abcdef01234567 # 1.80.0\n",
    )
}

pub(in crate::checks::pins::tests) fn quoted_keys_and_flow_mappings() -> Repo {
    workflow(
        "quoted-flow",
        "jobs:\n  a:\n    steps:\n\
         \x20     - \"uses\": 'pnpm/action-setup@v4'\n        \"with\": { \"version\": \"10\" }\n\
         \x20     - { uses: Actions/Setup-Node@v4, with: { Node-Version: 20 } }\n",
    )
}

pub(in crate::checks::pins::tests) fn anchors_aliases_and_merge_keys() -> Repo {
    workflow(
        "anchors",
        "x-pnpm: &pnpm\n  version: 10\nx-node: &node { node-version: 20 }\njobs:\n  a:\n    steps:\n\
         \x20     - uses: pnpm/action-setup@v4\n        with: *pnpm\n\
         \x20     - uses: pnpm/action-setup@v4\n        with: *pnpm\n\
         \x20     - uses: actions/setup-node@v4\n        with:\n          <<: *node\n",
    )
}

pub(in crate::checks::pins::tests) fn a_yaml_extension() -> Repo {
    with_file(
        "yaml-extension",
        ".github/workflows/release.yaml",
        "jobs:\n  a:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n\
         \x20         node-version: 20\n",
    )
}

pub(in crate::checks::pins::tests) fn versions_in_run_steps() -> Repo {
    workflow(
        "run-steps",
        "jobs:\n  a:\n    steps:\n      - run: npm i -g pnpm@7.8.9\n\
         \x20     - run: |\n          corepack enable\n          corepack prepare pnpm@7.8 --activate\n\
         \x20     - run: rustup toolchain install 3.4.5\n\
         \x20     - run: cargo install cargo-fake@5.6.7 lefthook@9.8.7\n",
    )
}

pub(in crate::checks::pins::tests) fn versions_in_other_values() -> Repo {
    workflow(
        "other-values",
        "env:\n  NODE: \"6.7\"\njobs:\n  a:\n    container: rust:3.4\n    steps:\n\
         \x20     - uses: docker://rust:3.4.5\n\
         \x20     - uses: taiki-e/install-action@v2\n        with:\n          tool: cargo-fake@5.6.7\n",
    )
}

pub(in crate::checks::pins::tests) fn pnpm_copy_in_a_run_step() -> Repo {
    workflow(
        "pnpm-copy-run",
        "jobs:\n  a:\n    steps:\n      - run: npm i -g pnpm@7.8.9\n",
    )
}

pub(in crate::checks::pins::tests) fn node_major_minor_in_a_block_scalar() -> Repo {
    workflow(
        "node-block-scalar",
        "jobs:\n  a:\n    steps:\n      - run: |\n          echo start\n          \
         fnm install 6.7 --corepack-enabled\n",
    )
}

pub(in crate::checks::pins::tests) fn rust_copy_in_a_flow_mapping() -> Repo {
    workflow(
        "rust-flow-mapping",
        "jobs:\n  a:\n    env: { RUSTUP_TOOLCHAIN: \"3.4.5\" }\n    steps:\n      - run: true\n",
    )
}

pub(in crate::checks::pins::tests) fn copy_under_a_quoted_key() -> Repo {
    workflow(
        "copy-quoted-key",
        "\"env\":\n  'Rustup_Toolchain': \"3.3\"\njobs:\n  a:\n    steps:\n      - run: true\n",
    )
}

pub(in crate::checks::pins::tests) fn copy_through_an_alias() -> Repo {
    workflow(
        "copy-alias",
        "x-rust: &rust \"rustup toolchain install 3.4.5\"\njobs:\n  a:\n    steps:\n\
         \x20     - run: *rust\n  b:\n    steps:\n      - run: *rust\n",
    )
}

pub(in crate::checks::pins::tests) fn copy_in_a_flow_sequence() -> Repo {
    workflow(
        "copy-flow-sequence",
        "jobs:\n  a:\n    strategy:\n      matrix:\n        image: [node:6.7.0]\n",
    )
}

pub(in crate::checks::pins::tests) fn an_upper_case_yml_extension() -> Repo {
    with_file(
        "upper-case-yml",
        ".github/workflows/CI.YML",
        "jobs:\n  a:\n    steps:\n      - run: rustup toolchain install 3.4.5\n",
    )
}

/// GitHub reads only the top level of `.github/workflows/`, and so does the check.
pub(in crate::checks::pins::tests) fn a_workflow_in_a_subfolder_passes() -> Repo {
    with_file(
        "workflow-subfolder",
        ".github/workflows/old/ci.yml",
        "jobs:\n  a:\n    steps:\n      - run: rustup toolchain install 3.4.5\n",
    )
}

pub(in crate::checks::pins::tests) fn workflow_not_yaml() -> Repo {
    workflow("workflow-not-yaml", "jobs:\n  a: {\n")
}
