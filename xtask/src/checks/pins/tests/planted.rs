//! The planted cases of the pins check: one `cases!` list, which makes `PLANTED` and one test
//! per case (see `checks/planted.rs`). The repos they run on are built in `tests/repos.rs`.

use crate::checks::pins::{FileHome, Fix};
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
    /// An engines.node in another form than `>=<major>.<minor>.<patch>` fails with the rule.
    engines_node_with_a_caret: Fails[failure(
        "could not read the minimum Node from ui/package.json: `engines.node` is `^6.7.0`; \
         engines.node takes only \">=<major>.<minor>.<patch>\"",
        Fix::Unread(FileHome::Node),
    )], inspects 1 => super::repos::engines_node_with_a_caret,
}
