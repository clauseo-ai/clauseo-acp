//! The planted repos of the forms that name a tool and a version in a workflow or an action:
//! where they are read, and the numbers that name no pinned tool.

use super::*;

/// Composite actions under `.github/actions/`, as `action.yml` and `action.yaml`, nested too.
pub(in crate::checks::pins::tests) fn pnpm_and_node_in_a_composite_action() -> Repo {
    Repo::files(
        "pins-composite-action",
        &homes(&[
            (
                ".github/actions/setup/action.yml",
                "name: setup\nruns:\n  using: composite\n  steps:\n\
                 \x20   - uses: pnpm/action-setup@v4\n      with:\n        version: 7\n\
                 \x20   - run: npm i -g pnpm@7.8.9\n      shell: bash\n",
            ),
            (
                ".github/actions/tools/node/action.yaml",
                "runs:\n  using: composite\n  steps:\n    - uses: actions/setup-node@v4\n\
                 \x20     with: { node-version: 6 }\n",
            ),
            (".github/actions/setup/notes.yml", "a: npm i -g pnpm@7\n"),
        ]),
    )
}

pub(in crate::checks::pins::tests) fn drifted_pnpm_and_node_container() -> Repo {
    workflow(
        "drift-npm-container",
        "jobs:\n  a:\n    container: node:6\n    services:\n      db:\n\
         \x20       image: docker.io/library/node:6.7-alpine\n    steps:\n\
         \x20     - run: npm i -g pnpm@7\n",
    )
}

/// The cargo tool is a real one in `TOOLS`, at a version it does not pin: the message names
/// the tool and the version written, never the pinned one.
pub(in crate::checks::pins::tests) fn corepack_and_cargo_install_versions() -> Repo {
    workflow(
        "corepack-cargo-install",
        "jobs:\n  a:\n    steps:\n      - run: corepack prepare pnpm@7.8.8 --activate\n\
         \x20     - run: |\n          cargo install --locked cargo-deny \\\n            \
         --version 0.0.1\n\
         \x20     - uses: taiki-e/install-action@v2\n        with:\n\
         \x20         tool: cargo-nextest@0.0.1\n",
    )
}

pub(in crate::checks::pins::tests) fn rustup_and_plus_toolchains() -> Repo {
    workflow(
        "rustup-forms",
        "jobs:\n  a:\n    steps:\n      - run: rustup default 3.3 && cargo +3.4.5 build\n\
         \x20     - run: rustup component add clippy --toolchain 3.4\n\
         \x20     - uses: docker://rust:3.4.5\n",
    )
}

/// A number that belongs to no pinned tool, even the Rust major.minor, and channels pass.
pub(in crate::checks::pins::tests) fn numbers_that_name_no_tool() -> Repo {
    workflow(
        "numbers-no-tool",
        "env:\n  NODE: \"6.7\"\njobs:\n  a:\n    steps:\n\
         \x20     - uses: actions/setup-python@v5\n        with:\n          python-version: '3.4'\n\
         \x20     - run: echo 3.4.5 7.8.9 && rustup toolchain install nightly\n\
         \x20     - run: cargo +nightly fuzz run a && npx pnpm@latest i\n",
    )
}

/// The toolchain a commit of dtolnay/rust-toolchain installs cannot be told without a comment.
pub(in crate::checks::pins::tests) fn dtolnay_at_a_bare_commit() -> Repo {
    workflow(
        "dtolnay-bare-commit",
        "jobs:\n  a:\n    steps:\n\
         \x20     - uses: dtolnay/rust-toolchain@0123456789abcdef0123456789abcdef01234567\n",
    )
}
