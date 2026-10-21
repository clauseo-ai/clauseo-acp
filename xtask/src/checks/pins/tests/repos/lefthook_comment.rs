//! The planted repos of `lefthook.yml`: its comments that name a lefthook version, a string or a
//! block scalar that holds one, and a file that is not YAML. Each is `repos.rs`'s `lefthook`.

use super::*;

/// The comment names the lefthook version the real `BINARIES` pins, read from it, since the
/// check runs as `cargo xtask check` does, with the real pins.
pub(in crate::checks::pins::tests) fn lefthook_comment_matches() -> Repo {
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

pub(in crate::checks::pins::tests) fn lefthook_comment_differs() -> Repo {
    lefthook(
        "lefthook-differs",
        "pre-commit:\n  commands:\n    # lefthook 9.8.6: `root` runs the command there.\n    \
         fmt: { run: cargo fmt }\n",
    )
}

pub(in crate::checks::pins::tests) fn lefthook_comment_after_a_value() -> Repo {
    lefthook(
        "lefthook-trailing",
        "pre-commit:\n  parallel: true # as of Lefthook v10.0:\n  commands:\n    \
         fmt: { run: \"cargo fmt # x\" } # lefthook 9.0.0\n",
    )
}

pub(in crate::checks::pins::tests) fn lefthook_text_in_a_string() -> Repo {
    lefthook(
        "lefthook-string",
        "pre-commit:\n  commands:\n    a: { run: \"echo '# lefthook 1.0.0'\" }\n    b:\n      \
         run: |\n        # lefthook 1.0.0 in a block scalar\n        true\n",
    )
}

pub(in crate::checks::pins::tests) fn lefthook_not_yaml() -> Repo {
    lefthook("lefthook-not-yaml", "pre-commit:\n  commands: [\n")
}

pub(in crate::checks::pins::tests) fn lefthook_comment_at_form() -> Repo {
    lefthook(
        "lefthook-at-form",
        "pre-commit:\n  commands: [fmt] #see LEFTHOOK@9.8.6\n",
    )
}
