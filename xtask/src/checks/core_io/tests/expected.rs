//! The expected text of the cfg rule's violations in `crates/protocol/src/hidden.rs`, as macros
//! the planted cases in `planted.rs` use.

/// The violation for a cfg with `$form` at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! cfg_form {
    ($line:literal, $form:literal) => {
        concat!(
            "crates/protocol/src/hidden.rs:",
            $line,
            ": `crates/protocol/src/hidden.rs` has a cfg \
             with ",
            $form,
            "; clippy lints clauseo-protocol once per target kind with every \
             feature on, so only a cfg built from `test` and positive features holds in one of \
             its runs, `doc` holds only when rustdoc documents the crate, and code behind any \
             other cfg can be left out of every clippy run\n    \
             fix: remove ",
            $form,
            " from this cfg; the cfgs of a crate whose row bans I/O use \
             only `test`, `doc` and features, joined by `all`, `any` and `not`, with no feature \
             under a `not`"
        )
    };
}

/// The violation for a `path` attribute at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! path_attribute {
    ($line:literal) => {
        concat!(
            "crates/protocol/src/hidden.rs:", $line, ": `crates/protocol/src/hidden.rs` gives a \
             module a `path` attribute that core-io cannot follow to a file it lexes, so it can \
             bring into clauseo-protocol a file core-io does not read\n    fix: give \
             `#[path = \"..\"]` only to an out-of-line `mod name;` at the top level of a file, \
             outside every brace, bracket and parenthesis, with a string literal that names, \
             relative to this file's folder and with `/` between components, a `.rs` file under \
             the crate's folder that git lists and that is not a symlink, without leaving that \
             folder on the way; a `path` in a `cfg_attr`, on an inline module, inside a module \
             block or with a metavariable value always fails"
        )
    };
}

/// The violation for an attribute whose name is a metavariable at line `$line` of
/// `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! metavariable_attribute {
    ($line:literal) => {
        concat!(
            "crates/protocol/src/hidden.rs:", $line, ": `crates/protocol/src/hidden.rs` writes an \
             attribute whose name is a metavariable, so a macro of clauseo-protocol can build a \
             cfg that core-io does not see\n    fix: write the attribute's name in the macro body \
             itself; an outer attribute may instead come whole from the caller, bound as \
             `#[$m:meta]` in the arm's matcher and written as `#[$m]`, in a `macro_rules!` \
             outside every other macro's body and arguments"
        )
    };
}

/// The violation for brackets joined to a `#` at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! joined_brackets {
    ($line:literal) => {
        concat!(
            "crates/protocol/src/hidden.rs:", $line, ": `crates/protocol/src/hidden.rs` writes \
             brackets right after a repetition or a metavariable that can stand for a `#`, so a \
             macro of clauseo-protocol can build an attribute that core-io does not see\n    \
             fix: put the brackets after a token the macro body writes itself, or after a \
             metavariable this arm binds as a fragment other than `tt`, such as `ident` or `expr`"
        )
    };
}

/// The violation for the name `cfg_select` at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! cfg_select {
    ($line:literal) => {
        concat!(
            "crates/protocol/src/hidden.rs:", $line, ": `crates/protocol/src/hidden.rs` names \
             `cfg_select`, a name a crate whose row bans I/O never uses, since `cfg_select!` \
             builds only the first arm whose cfg holds and a `use` can call it under another \
             name\n    fix: rename a field, variable or import named `cfg_select`; for the macro, \
             give each arm's code its own `#[cfg(..)]` built from `test`, `doc` and positive \
             features, or use `if cfg!(..)`, which compiles both branches"
        )
    };
}
