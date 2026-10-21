//! The tauri-config builders for the desktop crate's Rust sources and cargo config:
//! `generate_context!` and `TAURI_CONFIG`.

use super::*;

/// `generate_context!("other.json")` on line 2 of `src/lib.rs`.
pub(in crate::checks::tauri_config::tests) fn generate_context_with_a_config_path() -> Repo {
    tree_case(
        "context-config-path",
        &[(
            "crates/desktop/src/lib.rs",
            "pub fn run() {\n    let _context = tauri::generate_context!(\"other.json\");\n}\n",
        )],
    )
}

/// `src/ctx.rs` holds only an expression, as `include!` takes it: braced `generate_context!`
/// with `capabilities` on line 2. It is no whole Rust file, yet its tokens are read.
pub(in crate::checks::tauri_config::tests) fn generate_context_in_an_included_fragment() -> Repo {
    tree_case(
        "context-fragment",
        &[(
            "crates/desktop/src/ctx.rs",
            "// pulled in with include!\ntauri::generate_context! { capabilities = [\"x.json\"] }\n",
        )],
    )
}

/// `generate_context` imported as `ctx` on line 1 of a nested module file.
pub(in crate::checks::tauri_config::tests) fn generate_context_renamed() -> Repo {
    tree_case(
        "context-renamed",
        &[(
            "crates/desktop/src/app/mod.rs",
            "use tauri::generate_context as ctx;\n\npub fn run() {\n    let _c = ctx!(\"o.json\");\n}\n",
        )],
    )
}

/// `generate_context!()` with no argument, beside a doc comment and a string that show it with
/// one: the two Rust files are read and pass.
pub(in crate::checks::tauri_config::tests) fn generate_context_without_arguments_passes() -> Repo {
    tree_case(
        "context-clean",
        &[
            (
                "crates/desktop/src/lib.rs",
                "/// Not `generate_context!(\"other.json\")`.\npub fn run() {\n    \
                 let _c = tauri::generate_context!();\n    \
                 let _s = \"generate_context!(\\\"x\\\")\";\n}\n",
            ),
            (
                "crates/desktop/src/main.rs",
                "fn main() {\n    app::run();\n}\n",
            ),
        ],
    )
}

/// A Rust file of the desktop crate whose string never ends, so it does not lex.
pub(in crate::checks::tauri_config::tests) fn source_that_does_not_lex() -> Repo {
    tree_case(
        "source-syntax",
        &[(
            "crates/desktop/src/lib.rs",
            "pub fn run() {\n    \"open\n}\n",
        )],
    )
}

/// `.cargo/config.toml` sets `tauri_config` (lower case) in `[env]` on line 4.
pub(in crate::checks::tauri_config::tests) fn tauri_config_in_cargo_env() -> Repo {
    tree_case(
        "cargo-env",
        &[(
            ".cargo/config.toml",
            "[alias]\nx = \"run\"\n\n[env]\ntauri_config = { value = \"{}\", force = true }\n",
        )],
    )
}

/// `.cargo/config.toml` with another `[env]` entry and a dotted `env.TAURI_CONFIG_X`: the
/// file is read and passes.
pub(in crate::checks::tauri_config::tests) fn other_cargo_env_passes() -> Repo {
    tree_case(
        "cargo-env-clean",
        &[(
            ".cargo/config.toml",
            "env.TAURI_CONFIG_X = \"1\"\nenv.OTHER = \"{}\"\n",
        )],
    )
}
