//! The texts of the cfg rule's findings. Each one is the violation that a finding reports, with
//! its fix.

use crate::checks::core_io::cfgs::Finding;
use crate::checks::violation;
use crate::violation::Violation;

/// The violation for `finding` at `line` of `path`, a file of `package`.
pub(in crate::checks::core_io) fn cfg_rule(
    path: &str,
    line: u32,
    finding: &Finding,
    package: &str,
) -> Violation {
    let (message, fix) = match finding {
        Finding::Predicate(form) => (
            format!(
                "`{path}` has a cfg with {form}; clippy lints {package} once per target kind with \
                 every feature on, so only a cfg built from `test` and positive features holds in \
                 one of its runs, `doc` holds only when rustdoc documents the crate, and code \
                 behind any other cfg can be left out of every clippy run"
            ),
            format!(
                "remove {form} from this cfg; the cfgs of a crate whose row bans I/O use only \
                 `test`, `doc` and features, joined by `all`, `any` and `not`, with no feature \
                 under a `not`"
            ),
        ),
        Finding::MetavariableAttribute => (
            format!(
                "`{path}` writes an attribute whose name is a metavariable, so a macro of \
                 {package} can build a cfg that core-io does not see"
            ),
            "write the attribute's name in the macro body itself; an outer attribute may instead \
             come whole from the caller, bound as `#[$m:meta]` in the arm's matcher and written \
             as `#[$m]`, in a `macro_rules!` outside every other macro's body and arguments"
                .to_owned(),
        ),
        Finding::MetavariableMacro => (
            format!(
                "`{path}` calls a macro whose name is a metavariable, so a macro of {package} can \
                 call `include!` under a name passed to it"
            ),
            "write the called macro's name in the macro body itself".to_owned(),
        ),
        Finding::HashWithoutBrackets => (
            format!(
                "`{path}` writes a `#` that is followed by neither brackets nor `!` and \
                 brackets, so a macro can join it to brackets from elsewhere and build an \
                 attribute in {package} that core-io does not see"
            ),
            "write the attribute's brackets right after its `#`, as `#[..]` or `#![..]`; to \
             pass an attribute to a macro, pass it whole, with its `#` and its brackets"
                .to_owned(),
        ),
        Finding::PathAttribute => (
            format!(
                "`{path}` gives a module a `path` attribute that core-io cannot follow to a file \
                 it lexes, so it can bring into {package} a file core-io does not read"
            ),
            "give `#[path = \"..\"]` only to an out-of-line `mod name;` at the top level of a \
             file, outside every brace, bracket and parenthesis, with a string literal that \
             names, relative to this file's folder and with `/` between components, a `.rs` \
             file under the crate's folder that git lists and that is not a symlink, without \
             leaving that folder on the way; a `path` in a `cfg_attr`, on an inline module, \
             inside a module block or with a metavariable value always fails"
                .to_owned(),
        ),
        Finding::CfgSelect => (
            format!(
                "`{path}` names `cfg_select`, a name a crate whose row bans I/O never uses, since \
                 `cfg_select!` builds only the first arm whose cfg holds and a `use` can call it \
                 under another name"
            ),
            "rename a field, variable or import named `cfg_select`; for the macro, give each \
             arm's code its own `#[cfg(..)]` built from `test`, `doc` and positive features, or \
             use `if cfg!(..)`, which compiles both branches"
                .to_owned(),
        ),
        Finding::TtFragment => (
            format!(
                "`{path}` names `tt`, a name a crate whose row bans I/O never uses, since a \
                 `tt` fragment can take a lone `#` out of an attribute and a macro of {package} \
                 can join it to other brackets into an attribute that core-io does not see"
            ),
            "give the macros of a crate whose row bans I/O no `tt` fragment and use a typed \
             fragment such as `item`, `expr`, `ident`, `ty` or `meta`; rename an identifier \
             named `tt`"
                .to_owned(),
        ),
        Finding::CfgInCfgAttr => (
            format!(
                "`{path}` puts a `cfg` inside a `cfg_attr`, which keeps the item whenever the \
                 `cfg_attr`'s predicate is false, so a feature there acts like a `not(feature)` \
                 and can leave code out of every clippy run"
            ),
            "write the condition as one `#[cfg(..)]` outside any `cfg_attr`, built from `test`, \
             `doc` and positive features"
                .to_owned(),
        ),
        Finding::Include => (
            format!(
                "`{path}` calls `include!`, which can bring into {package} code from a file \
                 core-io does not read"
            ),
            "move the code into a module at its default path; use `include_str!` or \
             `include_bytes!` for data"
                .to_owned(),
        ),
    };
    violation(path, line, message, fix)
}
