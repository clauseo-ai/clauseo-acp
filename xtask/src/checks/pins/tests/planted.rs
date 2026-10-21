//! The planted cases of the pins check: one `cases!` list, which makes `PLANTED` and one test
//! per case (see `checks/planted.rs`). The repos they run on are built in `tests/repos.rs`.

use crate::checks::pins::{FileHome, Fix, Home};
use crate::checks::planted::{fails, failure};

crate::checks::planted::cases! {
    /// A rust-version with the toolchain's major.minor passes.
    rust_version_matches: Passes, inspects 1 => super::repos::rust_version_matches,
    /// A rust-version with another minor fails at its line, naming the home.
    rust_version_differs: Fails[fails(
        "Cargo.toml:3",
        "rust-version \"3.3\" is not the major.minor of the Rust version `channel` in \
         rust-toolchain.toml pins (3.4.5)",
        Fix::RustVersion {
            channel_major_minor: "3.4".to_owned(),
        },
    )], inspects 1 => super::repos::rust_version_differs,
    /// A dotted `package.rust-version` key under `[workspace]` is read too.
    rust_version_in_a_dotted_key: Fails[fails(
        "Cargo.toml:3",
        "rust-version \"3.5.0\" is not the major.minor of the Rust version `channel` in \
         rust-toolchain.toml pins (3.4.5)",
        Fix::RustVersion {
            channel_major_minor: "3.4".to_owned(),
        },
    )], inspects 1 => super::repos::rust_version_in_a_dotted_key,
    /// A member's own `[package] rust-version` is a copy too.
    rust_version_in_a_member: Fails[fails(
        "crates/desktop/Cargo.toml:3",
        "rust-version \"2.4\" is not the major.minor of the Rust version `channel` in \
         rust-toolchain.toml pins (3.4.5)",
        Fix::RustVersion {
            channel_major_minor: "3.4".to_owned(),
        },
    )], inspects 2 => super::repos::rust_version_in_a_member,
    /// A member that inherits rust-version holds no copy.
    rust_version_inherited_by_a_member: Passes,
        inspects 2 => super::repos::rust_version_inherited_by_a_member,
    /// A manifest that is not TOML fails at the line the parser stopped on.
    manifest_not_toml: Fails[fails(
        "Cargo.toml:1",
        "Cargo.toml is not TOML: unclosed table, expected `]`",
        Fix::NotToml,
    )], inspects 1 => super::repos::manifest_not_toml,
    /// With no rust-toolchain.toml there is no home to compare with: a failure with no line.
    toolchain_missing: Fails[failure(
        "could not read the Rust version from rust-toolchain.toml: the file does not exist",
        Fix::Unread(FileHome::Rust),
    )], inspects 1 => super::repos::toolchain_missing,
    /// A channel that names no version, such as `stable`, is no home either.
    channel_not_a_version: Fails[failure(
        "could not read the Rust version from rust-toolchain.toml: `channel = \"stable\"` is not \
         a version such as 1.2.3",
        Fix::Unread(FileHome::Rust),
    )], inspects 1 => super::repos::channel_not_a_version,
    /// A packageManager that is not pnpm gives no pnpm version.
    package_manager_not_pnpm: Fails[failure(
        "could not read the pnpm version from ui/package.json: `packageManager` is `yarn@1.2.3`, \
         not `pnpm@<major>.<minor>.<patch>`",
        Fix::Unread(FileHome::Pnpm),
    )], inspects 1 => super::repos::package_manager_not_pnpm,
    /// A package.json with no engines.node gives no minimum Node.
    engines_node_missing: Fails[failure(
        "could not read the minimum Node from ui/package.json: it has no `engines.node` string",
        Fix::Unread(FileHome::Node),
    )], inspects 1 => super::repos::engines_node_missing,
    /// A comment that names the pinned lefthook passes.
    lefthook_comment_matches: Passes, inspects 2 => super::repos::lefthook_comment_matches,
    /// `# lefthook` inside a quoted string or a block scalar is text, not a comment.
    lefthook_text_in_a_string: Passes, inspects 2 => super::repos::lefthook_text_in_a_string,
    /// A lefthook.yml that is not YAML fails at the line the parser stopped on.
    lefthook_not_yaml: Fails[fails(
        "lefthook.yml:3",
        "lefthook.yml is not YAML: while parsing a node, did not find expected node content",
        Fix::NotYaml,
    )], inspects 2 => super::repos::lefthook_not_yaml,
    /// A workflow that reads every version from its home passes; a SHA ref's comment and a
    /// version inside a longer number are not copies.
    workflow_reads_the_homes: Passes, inspects 2 => super::repos::workflow_reads_the_homes,
    /// A `version` input to pnpm/action-setup fails at its key.
    pnpm_version_input: Fails[fails(
        ".github/workflows/ci.yml:6",
        "`version` tells pnpm/action-setup which pnpm to install; the one home of that version is \
         `packageManager` in ui/package.json",
        Fix::Input(FileHome::Pnpm),
    )], inspects 2 => super::repos::pnpm_version_input,
    /// A `node-version` input to actions/setup-node fails, whatever its value.
    node_version_input: Fails[fails(
        ".github/workflows/ci.yml:6",
        "`node-version` tells actions/setup-node which Node to install; the one home of that \
         version is `engines.node` in ui/package.json",
        Fix::Input(FileHome::Node),
    )], inspects 2 => super::repos::node_version_input,
    /// The toolchain input of each Rust toolchain action fails.
    toolchain_inputs: Fails[
        fails(
            ".github/workflows/ci.yml:6",
            "`toolchain` tells dtolnay/rust-toolchain which Rust toolchain to install; the one \
             home of that version is `channel` in rust-toolchain.toml",
            Fix::Input(FileHome::Rust),
        ),
        fails(
            ".github/workflows/ci.yml:9",
            "`toolchain` tells actions-rust-lang/setup-rust-toolchain which Rust toolchain to \
             install; the one home of that version is `channel` in rust-toolchain.toml",
            Fix::Input(FileHome::Rust),
        ),
        fails(
            ".github/workflows/ci.yml:12",
            "`toolchain` tells actions-rs/toolchain which Rust toolchain to install; the one home \
             of that version is `channel` in rust-toolchain.toml",
            Fix::Input(FileHome::Rust),
        ),
        fails(
            ".github/workflows/ci.yml:15",
            "`channel` tells moonrepo/setup-rust which Rust toolchain to install; the one home of \
             that version is `channel` in rust-toolchain.toml",
            Fix::Input(FileHome::Rust),
        ),
    ], inspects 2 => super::repos::toolchain_inputs,
    /// dtolnay/rust-toolchain at a version ref fails.
    toolchain_in_the_ref: Fails[fails(
        ".github/workflows/ci.yml:4",
        "`uses: dtolnay/rust-toolchain@1.80.0` tells dtolnay/rust-toolchain which Rust toolchain \
         to install through its ref; the one home of that version is `channel` in \
         rust-toolchain.toml",
        Fix::Input(FileHome::Rust),
    )], inspects 2 => super::repos::toolchain_in_the_ref,
    /// dtolnay/rust-toolchain at a commit whose comment names a version fails.
    toolchain_in_a_commit_comment: Fails[fails(
        ".github/workflows/ci.yml:4",
        "`uses: dtolnay/rust-toolchain@0123456789abcdef0123456789abcdef01234567` pins \
         dtolnay/rust-toolchain by commit, and the comment on its line names 1.80.0, the Rust \
         toolchain a commit of its `1.80.0` branch installs; the one home of that version is \
         `channel` in rust-toolchain.toml",
        Fix::Input(FileHome::Rust),
    )], inspects 2 => super::repos::toolchain_in_a_commit_comment,
    /// Quoted keys, flow mappings and other cases of the action and input names fail.
    quoted_keys_and_flow_mappings: Fails[
        fails(
            ".github/workflows/ci.yml:5",
            "`version` tells pnpm/action-setup which pnpm to install; the one home of that version \
             is `packageManager` in ui/package.json",
            Fix::Input(FileHome::Pnpm),
        ),
        fails(
            ".github/workflows/ci.yml:6",
            "`Node-Version` tells actions/setup-node which Node to install; the one home of that \
             version is `engines.node` in ui/package.json",
            Fix::Input(FileHome::Node),
        ),
    ], inspects 2 => super::repos::quoted_keys_and_flow_mappings,
    /// Inputs through an alias or a merge key fail once, at the anchored key.
    anchors_aliases_and_merge_keys: Fails[
        fails(
            ".github/workflows/ci.yml:2",
            "`version` tells pnpm/action-setup which pnpm to install; the one home of that version \
             is `packageManager` in ui/package.json",
            Fix::Input(FileHome::Pnpm),
        ),
        fails(
            ".github/workflows/ci.yml:3",
            "`node-version` tells actions/setup-node which Node to install; the one home of that \
             version is `engines.node` in ui/package.json",
            Fix::Input(FileHome::Node),
        ),
    ], inspects 2 => super::repos::anchors_aliases_and_merge_keys,
    /// A `.yaml` workflow is read too.
    a_yaml_extension: Fails[fails(
        ".github/workflows/release.yaml:6",
        "`node-version` tells actions/setup-node which Node to install; the one home of that \
         version is `engines.node` in ui/package.json",
        Fix::Input(FileHome::Node),
    )], inspects 2 => super::repos::a_yaml_extension,
    /// A workflow that is not YAML fails at the line the parser stopped on.
    workflow_not_yaml: Fails[fails(
        ".github/workflows/ci.yml:3",
        ".github/workflows/ci.yml is not YAML: while parsing a node, did not find expected node \
         content",
        Fix::NotYaml,
    )], inspects 2 => super::repos::workflow_not_yaml,
    /// The pnpm version in a run step is a copy of its home and fails at its line.
    pnpm_copy_in_a_run_step: Fails[fails(
        ".github/workflows/ci.yml:4",
        "`pnpm@7.8.9` names pnpm 7.8.9; the one home of the pnpm version is `packageManager` in \
         ui/package.json",
        Fix::Copy(Home::File(FileHome::Pnpm)),
    )], inspects 2 => super::repos::pnpm_copy_in_a_run_step,
    /// The Node major.minor on a block scalar's third line fails at that line.
    node_major_minor_in_a_block_scalar: Fails[fails(
        ".github/workflows/ci.yml:6",
        "`fnm install 6.7` names Node 6.7; the one home of the minimum Node is `engines.node` in \
         ui/package.json",
        Fix::Copy(Home::File(FileHome::Node)),
    )], inspects 2 => super::repos::node_major_minor_in_a_block_scalar,
    /// A copy in a flow mapping fails at its line.
    rust_copy_in_a_flow_mapping: Fails[fails(
        ".github/workflows/ci.yml:3",
        "`RUSTUP_TOOLCHAIN: 3.4.5` names Rust toolchain 3.4.5; the one home of the Rust version is \
         `channel` in rust-toolchain.toml",
        Fix::Copy(Home::File(FileHome::Rust)),
    )], inspects 2 => super::repos::rust_copy_in_a_flow_mapping,
    /// A copy under a quoted key fails: keys are matched as YAML reads them.
    copy_under_a_quoted_key: Fails[fails(
        ".github/workflows/ci.yml:2",
        "`Rustup_Toolchain: 3.3` names Rust toolchain 3.3; the one home of the Rust version is \
         `channel` in rust-toolchain.toml",
        Fix::Copy(Home::File(FileHome::Rust)),
    )], inspects 2 => super::repos::copy_under_a_quoted_key,
    /// A copy behind an anchor fails once, at the anchored value, however often it is used.
    copy_through_an_alias: Fails[fails(
        ".github/workflows/ci.yml:1",
        "`rustup toolchain install 3.4.5` names Rust toolchain 3.4.5; the one home of the Rust \
         version is `channel` in rust-toolchain.toml",
        Fix::Copy(Home::File(FileHome::Rust)),
    )], inspects 2 => super::repos::copy_through_an_alias,
    /// A copy in a flow sequence fails at its line.
    copy_in_a_flow_sequence: Fails[fails(
        ".github/workflows/ci.yml:5",
        "`node:6.7.0` names Node 6.7.0; the one home of the minimum Node is `engines.node` in \
         ui/package.json",
        Fix::Copy(Home::File(FileHome::Node)),
    )], inspects 2 => super::repos::copy_in_a_flow_sequence,
    /// A workflow with an upper-case `.YML` extension is read too.
    an_upper_case_yml_extension: Fails[fails(
        ".github/workflows/CI.YML:4",
        "`rustup toolchain install 3.4.5` names Rust toolchain 3.4.5; the one home of the Rust \
         version is `channel` in rust-toolchain.toml",
        Fix::Copy(Home::File(FileHome::Rust)),
    )], inspects 2 => super::repos::an_upper_case_yml_extension,
    /// A workflow in a subfolder is not one GitHub runs, so it is not read.
    a_workflow_in_a_subfolder_passes: Passes,
        inspects 1 => super::repos::a_workflow_in_a_subfolder_passes,
    /// The root manifest's own `[package] rust-version` is a copy too.
    rust_version_in_a_root_package: Fails[fails(
        "Cargo.toml:3",
        "rust-version \"3.3\" is not the major.minor of the Rust version `channel` in \
         rust-toolchain.toml pins (3.4.5)",
        Fix::RustVersion {
            channel_major_minor: "3.4".to_owned(),
        },
    )], inspects 1 => super::repos::rust_version_in_a_root_package,
    /// A member's quoted `"rust-version"` key in a quoted table is read.
    rust_version_in_a_quoted_member_key: Fails[fails(
        "crates/desktop/Cargo.toml:3",
        "rust-version \"2.4\" is not the major.minor of the Rust version `channel` in \
         rust-toolchain.toml pins (3.4.5)",
        Fix::RustVersion {
            channel_major_minor: "3.4".to_owned(),
        },
    )], inspects 2 => super::repos::rust_version_in_a_quoted_member_key,
    /// A member whose package is an inline table is read.
    rust_version_in_an_inline_member_table: Fails[fails(
        "crates/desktop/Cargo.toml:1",
        "rust-version \"2.4\" is not the major.minor of the Rust version `channel` in \
         rust-toolchain.toml pins (3.4.5)",
        Fix::RustVersion {
            channel_major_minor: "3.4".to_owned(),
        },
    )], inspects 2 => super::repos::rust_version_in_an_inline_member_table,
    /// Composite actions are read like workflows: a version input and a pnpm version fail.
    pnpm_and_node_in_a_composite_action: Fails[
        fails(
            ".github/actions/setup/action.yml:7",
            "`version` tells pnpm/action-setup which pnpm to install; the one home of that version \
             is `packageManager` in ui/package.json",
            Fix::Input(FileHome::Pnpm),
        ),
        fails(
            ".github/actions/setup/action.yml:8",
            "`pnpm@7.8.9` names pnpm 7.8.9; the one home of the pnpm version is `packageManager` in \
             ui/package.json",
            Fix::Copy(Home::File(FileHome::Pnpm)),
        ),
        fails(
            ".github/actions/tools/node/action.yaml:5",
            "`node-version` tells actions/setup-node which Node to install; the one home of that \
             version is `engines.node` in ui/package.json",
            Fix::Input(FileHome::Node),
        ),
    ], inspects 3 => super::repos::pnpm_and_node_in_a_composite_action,
    /// Versions that drift from the homes fail too: a pnpm major, Node container images.
    drifted_pnpm_and_node_container: Fails[
        fails(
            ".github/workflows/ci.yml:3",
            "`node:6` names Node 6; the one home of the minimum Node is `engines.node` in \
             ui/package.json",
            Fix::Copy(Home::File(FileHome::Node)),
        ),
        fails(
            ".github/workflows/ci.yml:6",
            "`node:6.7-alpine` names Node 6.7-alpine; the one home of the minimum Node is \
             `engines.node` in ui/package.json",
            Fix::Copy(Home::File(FileHome::Node)),
        ),
        fails(
            ".github/workflows/ci.yml:8",
            "`pnpm@7` names pnpm 7; the one home of the pnpm version is `packageManager` in \
             ui/package.json",
            Fix::Copy(Home::File(FileHome::Pnpm)),
        ),
    ], inspects 2 => super::repos::drifted_pnpm_and_node_container,
    /// corepack, `cargo install --version` across a line break and an install action's tool.
    corepack_and_cargo_install_versions: Fails[
        fails(
            ".github/workflows/ci.yml:4",
            "`pnpm@7.8.8` names pnpm 7.8.8; the one home of the pnpm version is `packageManager` in \
             ui/package.json",
            Fix::Copy(Home::File(FileHome::Pnpm)),
        ),
        fails(
            ".github/workflows/ci.yml:7",
            "`cargo install --locked cargo-deny --version 0.0.1` names cargo-deny 0.0.1; the one \
             home of the cargo-deny version is `TOOLS` in xtask/src/setup.rs",
            Fix::Copy(Home::Tool("cargo-deny")),
        ),
        fails(
            ".github/workflows/ci.yml:10",
            "`cargo-nextest@0.0.1` names cargo-nextest 0.0.1; the one home of the cargo-nextest \
             version is `TOOLS` in xtask/src/setup.rs",
            Fix::Copy(Home::Tool("cargo-nextest")),
        ),
    ], inspects 2 => super::repos::corepack_and_cargo_install_versions,
    /// rustup's toolchain arguments, `cargo +<v>` and a Rust image.
    rustup_and_plus_toolchains: Fails[
        fails(
            ".github/workflows/ci.yml:4",
            "`rustup default 3.3` names Rust toolchain 3.3; the one home of the Rust version is \
             `channel` in rust-toolchain.toml",
            Fix::Copy(Home::File(FileHome::Rust)),
        ),
        fails(
            ".github/workflows/ci.yml:4",
            "`cargo +3.4.5` names Rust toolchain 3.4.5; the one home of the Rust version is \
             `channel` in rust-toolchain.toml",
            Fix::Copy(Home::File(FileHome::Rust)),
        ),
        fails(
            ".github/workflows/ci.yml:5",
            "`rustup component add clippy --toolchain 3.4` names Rust toolchain 3.4; the one home \
             of the Rust version is `channel` in rust-toolchain.toml",
            Fix::Copy(Home::File(FileHome::Rust)),
        ),
        fails(
            ".github/workflows/ci.yml:6",
            "`rust:3.4.5` names Rust toolchain 3.4.5; the one home of the Rust version is \
             `channel` in rust-toolchain.toml",
            Fix::Copy(Home::File(FileHome::Rust)),
        ),
    ], inspects 2 => super::repos::rustup_and_plus_toolchains,
    /// A number that names no pinned tool is not a copy: `python-version: '3.4'` passes.
    numbers_that_name_no_tool: Passes, inspects 2 => super::repos::numbers_that_name_no_tool,
    /// dtolnay/rust-toolchain at a commit with no comment cannot be checked, so it passes.
    dtolnay_at_a_bare_commit: Passes, inspects 2 => super::repos::dtolnay_at_a_bare_commit,
    /// An engines.node in another form than `>=<major>.<minor>.<patch>` fails with the rule.
    engines_node_with_a_caret: Fails[failure(
        "could not read the minimum Node from ui/package.json: `engines.node` is `^6.7.0`; \
         engines.node takes only \">=<major>.<minor>.<patch>\"",
        Fix::Unread(FileHome::Node),
    )], inspects 1 => super::repos::engines_node_with_a_caret,
}
