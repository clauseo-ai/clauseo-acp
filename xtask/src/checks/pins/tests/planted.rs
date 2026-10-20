//! The planted cases of the pins check: one `cases!` list, which makes `PLANTED` and one test
//! per case (see `checks/planted.rs`). The repos they run on are built in `tests/cases.rs`.

crate::checks::planted::cases! {
    /// A rust-version with the toolchain's major.minor passes.
    rust_version_matches: Passes, inspects 1 => super::cases::rust_version_matches,
    /// A rust-version with another minor fails at its line, naming the home.
    rust_version_differs: FailsWith(&[
        "Cargo.toml:3: rust-version \"3.3\" is not the major.minor of the Rust version `channel` \
         in rust-toolchain.toml pins (3.4.5)\n    fix: set rust-version to \"3.4\", the \
         major.minor of `channel` in rust-toolchain.toml, the one home of the Rust version",
    ]), inspects 1 => super::cases::rust_version_differs,
    /// A dotted `package.rust-version` key under `[workspace]` is read too.
    rust_version_in_a_dotted_key: FailsWith(&[
        "Cargo.toml:3: rust-version \"3.5.0\" is not the major.minor of the Rust version \
         `channel` in rust-toolchain.toml pins (3.4.5)\n    fix: set rust-version to \"3.4\", \
         the major.minor of `channel` in rust-toolchain.toml, the one home of the Rust version",
    ]), inspects 1 => super::cases::rust_version_in_a_dotted_key,
    /// A member's own `[package] rust-version` is a copy too.
    rust_version_in_a_member: FailsWith(&[
        "crates/desktop/Cargo.toml:3: rust-version \"2.4\" is not the major.minor of the Rust \
         version `channel` in rust-toolchain.toml pins (3.4.5)\n    fix: set rust-version to \
         \"3.4\", the major.minor of `channel` in rust-toolchain.toml, the one home of the Rust \
         version",
    ]), inspects 2 => super::cases::rust_version_in_a_member,
    /// A member that inherits rust-version holds no copy.
    rust_version_inherited_by_a_member: Passes,
        inspects 2 => super::cases::rust_version_inherited_by_a_member,
    /// A manifest that is not TOML fails at the line the parser stopped on.
    manifest_not_toml: FailsWith(&[
        "Cargo.toml:1: Cargo.toml is not TOML: unclosed table, expected `]`\n    fix: make the \
         manifest valid TOML at this line: pins reads its rust-version there",
    ]), inspects 1 => super::cases::manifest_not_toml,
    /// With no rust-toolchain.toml there is no home to compare with: a failure with no line.
    toolchain_missing: FailsWith(&[
        "could not read the Rust version from rust-toolchain.toml: the file does not exist\n    \
         fix: set `channel = \"<major>.<minor>.<patch>\"` under `[toolchain]` in \
         rust-toolchain.toml: it is the one home of the Rust version, and pins compares its \
         copies with it",
    ]), inspects 1 => super::cases::toolchain_missing,
    /// A channel that names no version, such as `stable`, is no home either.
    channel_not_a_version: FailsWith(&[
        "could not read the Rust version from rust-toolchain.toml: `channel = \"stable\"` is not \
         a version such as 1.2.3\n    fix: set `channel = \"<major>.<minor>.<patch>\"` under \
         `[toolchain]` in rust-toolchain.toml: it is the one home of the Rust version, and pins \
         compares its copies with it",
    ]), inspects 1 => super::cases::channel_not_a_version,
    /// A packageManager that is not pnpm gives no pnpm version.
    package_manager_not_pnpm: FailsWith(&[
        "could not read the pnpm version from ui/package.json: `packageManager` is `yarn@1.2.3`, \
         not `pnpm@<major>.<minor>.<patch>`\n    fix: set `\"packageManager\": \
         \"pnpm@<major>.<minor>.<patch>\"` in ui/package.json: it is the one home of the pnpm \
         version, and pins looks for copies of it",
    ]), inspects 1 => super::cases::package_manager_not_pnpm,
    /// A package.json with no engines.node gives no minimum Node.
    engines_node_missing: FailsWith(&[
        "could not read the minimum Node from ui/package.json: it has no `engines.node` string\n    \
         fix: set `\"engines\": { \"node\": \">=<major>.<minor>.<patch>\" }` in \
         ui/package.json: it is the one home of the minimum Node, and pins looks for copies of it",
    ]), inspects 1 => super::cases::engines_node_missing,
    /// A comment that names the pinned lefthook passes.
    lefthook_comment_matches: Passes, inspects 2 => super::cases::lefthook_comment_matches,
    /// `# lefthook` inside a quoted string or a block scalar is text, not a comment.
    lefthook_text_in_a_string: Passes, inspects 2 => super::cases::lefthook_text_in_a_string,
    /// A lefthook.yml that is not YAML fails at the line the parser stopped on.
    lefthook_not_yaml: FailsWith(&[
        "lefthook.yml:3: lefthook.yml is not YAML: while parsing a node, did not find expected \
         node content\n    fix: make the file valid YAML at this line: pins parses it to find \
         the versions it holds",
    ]), inspects 2 => super::cases::lefthook_not_yaml,
    /// A workflow that reads every version from its home passes; a SHA ref's comment and a
    /// version inside a longer number are not copies.
    workflow_reads_the_homes: Passes, inspects 2 => super::cases::workflow_reads_the_homes,
    /// A `version` input to pnpm/action-setup fails at its key.
    pnpm_version_input: FailsWith(&[
        ".github/workflows/ci.yml:6: `version` tells pnpm/action-setup which pnpm to install; the \
         one home of that version is `packageManager` in ui/package.json\n    fix: remove \
         `version` and set `package_json_file: ui/package.json`: pnpm/action-setup then installs \
         the pnpm `packageManager` names there, its one home",
    ]), inspects 2 => super::cases::pnpm_version_input,
    /// A `node-version` input to actions/setup-node fails, whatever its value.
    node_version_input: FailsWith(&[
        ".github/workflows/ci.yml:6: `node-version` tells actions/setup-node which Node to \
         install; the one home of that version is `engines.node` in ui/package.json\n    fix: \
         remove `node-version` and set `node-version-file: ui/package.json`: actions/setup-node \
         then installs a Node that `engines.node` there allows, its one home",
    ]), inspects 2 => super::cases::node_version_input,
    /// The toolchain input of each Rust toolchain action fails.
    toolchain_inputs: FailsWith(&[
        ".github/workflows/ci.yml:6: `toolchain` tells dtolnay/rust-toolchain which Rust \
         toolchain to install; the one home of that version is `channel` in \
         rust-toolchain.toml\n    fix: install Rust with a `run: rustup toolchain install` step \
         instead: with no argument rustup installs the toolchain rust-toolchain.toml names, the \
         one home of the Rust version",
        ".github/workflows/ci.yml:9: `toolchain` tells actions-rust-lang/setup-rust-toolchain \
         which Rust toolchain to install; the one home of that version is `channel` in \
         rust-toolchain.toml\n    fix: install Rust with a `run: rustup toolchain install` step \
         instead: with no argument rustup installs the toolchain rust-toolchain.toml names, the \
         one home of the Rust version",
        ".github/workflows/ci.yml:12: `toolchain` tells actions-rs/toolchain which Rust toolchain \
         to install; the one home of that version is `channel` in rust-toolchain.toml\n    fix: \
         install Rust with a `run: rustup toolchain install` step instead: with no argument \
         rustup installs the toolchain rust-toolchain.toml names, the one home of the Rust \
         version",
        ".github/workflows/ci.yml:15: `channel` tells moonrepo/setup-rust which Rust toolchain to \
         install; the one home of that version is `channel` in rust-toolchain.toml\n    fix: \
         install Rust with a `run: rustup toolchain install` step instead: with no argument \
         rustup installs the toolchain rust-toolchain.toml names, the one home of the Rust \
         version",
    ]), inspects 2 => super::cases::toolchain_inputs,
    /// dtolnay/rust-toolchain at a version ref fails.
    toolchain_in_the_ref: FailsWith(&[
        ".github/workflows/ci.yml:4: `uses: dtolnay/rust-toolchain@1.80.0` tells \
         dtolnay/rust-toolchain which Rust toolchain to install through its ref; the one home of \
         that version is `channel` in rust-toolchain.toml\n    fix: install Rust with a `run: \
         rustup toolchain install` step instead: with no argument rustup installs the toolchain \
         rust-toolchain.toml names, the one home of the Rust version",
    ]), inspects 2 => super::cases::toolchain_in_the_ref,
    /// dtolnay/rust-toolchain at a commit whose comment names a version fails.
    toolchain_in_a_commit_comment: FailsWith(&[
        ".github/workflows/ci.yml:4: `uses: \
         dtolnay/rust-toolchain@0123456789abcdef0123456789abcdef01234567` pins \
         dtolnay/rust-toolchain by commit, and the comment on its line names 1.80.0, the Rust \
         toolchain a commit of its `1.80.0` branch installs; the one home of that version is \
         `channel` in rust-toolchain.toml\n    fix: install Rust with a `run: rustup toolchain \
         install` step instead: with no argument rustup installs the toolchain \
         rust-toolchain.toml names, the one home of the Rust version",
    ]), inspects 2 => super::cases::toolchain_in_a_commit_comment,
    /// Quoted keys, flow mappings and other cases of the action and input names fail.
    quoted_keys_and_flow_mappings: FailsWith(&[
        ".github/workflows/ci.yml:5: `version` tells pnpm/action-setup which pnpm to install; the \
         one home of that version is `packageManager` in ui/package.json\n    fix: remove \
         `version` and set `package_json_file: ui/package.json`: pnpm/action-setup then installs \
         the pnpm `packageManager` names there, its one home",
        ".github/workflows/ci.yml:6: `Node-Version` tells actions/setup-node which Node to \
         install; the one home of that version is `engines.node` in ui/package.json\n    fix: \
         remove `node-version` and set `node-version-file: ui/package.json`: actions/setup-node \
         then installs a Node that `engines.node` there allows, its one home",
    ]), inspects 2 => super::cases::quoted_keys_and_flow_mappings,
    /// Inputs through an alias or a merge key fail once, at the anchored key.
    anchors_aliases_and_merge_keys: FailsWith(&[
        ".github/workflows/ci.yml:2: `version` tells pnpm/action-setup which pnpm to install; the \
         one home of that version is `packageManager` in ui/package.json\n    fix: remove \
         `version` and set `package_json_file: ui/package.json`: pnpm/action-setup then installs \
         the pnpm `packageManager` names there, its one home",
        ".github/workflows/ci.yml:3: `node-version` tells actions/setup-node which Node to \
         install; the one home of that version is `engines.node` in ui/package.json\n    fix: \
         remove `node-version` and set `node-version-file: ui/package.json`: actions/setup-node \
         then installs a Node that `engines.node` there allows, its one home",
    ]), inspects 2 => super::cases::anchors_aliases_and_merge_keys,
    /// A `.yaml` workflow is read too.
    a_yaml_extension: FailsWith(&[
        ".github/workflows/release.yaml:6: `node-version` tells actions/setup-node which Node to \
         install; the one home of that version is `engines.node` in ui/package.json\n    fix: \
         remove `node-version` and set `node-version-file: ui/package.json`: actions/setup-node \
         then installs a Node that `engines.node` there allows, its one home",
    ]), inspects 2 => super::cases::a_yaml_extension,
    /// A workflow that is not YAML fails at the line the parser stopped on.
    workflow_not_yaml: FailsWith(&[
        ".github/workflows/ci.yml:3: .github/workflows/ci.yml is not YAML: while parsing a node, \
         did not find expected node content\n    fix: make the file valid YAML at this line: \
         pins parses it to find the versions it holds",
    ]), inspects 2 => super::cases::workflow_not_yaml,
}
