//! Tests of `forms::named`: where a value names a pinned tool and a version, with the cases'
//! made-up `TOOLS` and `BINARIES`.

use super::cases::{BINARIES, TOOLS};
use super::*;

/// The words each form shows, by the text it is found in.
fn shown(text: &str) -> Vec<String> {
    forms::named(text, TOOLS, BINARIES)
        .into_iter()
        .map(|named| named.shown)
        .collect()
}

#[test]
fn each_form_names_its_tool_and_version() {
    let pnpm = Home::File(FileHome::Pnpm);
    let found = forms::named("corepack prepare pnpm@7.8.8 --activate", TOOLS, BINARIES);
    assert_eq!(
        found,
        [forms::Named {
            shown: "pnpm@7.8.8".to_owned(),
            version: "7.8.8".to_owned(),
            home: pnpm,
        }]
    );
    let cases: [(&str, &[&str]); 14] = [
        ("npm i -g pnpm@7", &["pnpm@7"]),
        (
            "volta install Node@v6 lefthook@9.8.6",
            &["Node@v6", "lefthook@9.8.6"],
        ),
        ("tool: cargo-fake@5.6.0", &["cargo-fake@5.6.0"]),
        ("node:6", &["node:6"]),
        (
            "docker://docker.io/library/node:6-alpine",
            &["node:6-alpine"],
        ),
        ("rust:3.4", &["rust:3.4"]),
        (
            "rustup toolchain install --profile minimal 3.4.4 -c clippy",
            &["rustup toolchain install --profile minimal 3.4.4"],
        ),
        (
            "rustup default 3.3 && rustup run 3.2 cargo -j 4",
            &["rustup default 3.3", "rustup run 3.2"],
        ),
        (
            "rustup component add clippy --toolchain=3.1",
            &["rustup component add clippy --toolchain=3.1"],
        ),
        ("rustup override set 3.0", &["rustup override set 3.0"]),
        (
            "cargo +3.4.4 build; rustc +3.4 x.rs",
            &["cargo +3.4.4", "rustc +3.4"],
        ),
        (
            "fnm install 6.7 && nvm use 6",
            &["fnm install 6.7", "nvm use 6"],
        ),
        (
            "cargo install --locked cargo-fake --version 5.6.0",
            &["cargo install --locked cargo-fake --version 5.6.0"],
        ),
        (
            "~/.cargo/bin/cargo binstall --version=9.0.0 lefthook \\\n  --no-confirm",
            &["~/.cargo/bin/cargo binstall --version=9.0.0 lefthook"],
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(shown(text), expected, "{text}");
    }
}

/// A number that names no pinned tool, a channel or a tag, and a name inside a longer one are
/// not read.
#[test]
fn other_numbers_channels_and_longer_names_are_not_read() {
    for text in [
        "3.4",
        "python-version: '3.4'",
        "pnpm@latest node:lts rust:slim",
        "rustup toolchain install stable nightly",
        "rustup run nightly cargo build -j 4",
        "cargo +nightly fuzz",
        "cargo install cargo-other --version 5.6.7",
        "cargo install cargo-fake",
        "cargo build --version 1.0",
        "fnm install --lts",
        "mypnpm@7 libnode:6 my-rust:3.4 a.node@6",
        "pnpm install --frozen-lockfile",
        "node: 6",
        "echo rustup; install 3.4",
    ] {
        assert_eq!(shown(text), Vec::<String>::new(), "{text}");
    }
}
