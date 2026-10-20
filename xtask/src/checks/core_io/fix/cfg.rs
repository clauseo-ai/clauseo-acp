//! The fix lines of the cfg rule's findings (`report/cfg.rs`), one variant per failure kind,
//! wrapped in `core_io::Fix::Cfg`. `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the cfg rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CfgFix {
    /// A cfg predicate of a form the rule does not allow, described as `form`.
    Predicate { form: String },
    /// An attribute whose name is a metavariable.
    MetavariableAttribute,
    /// A macro call whose name is a metavariable.
    MetavariableMacro,
    /// A `#` followed by neither brackets nor `!` and brackets.
    HashWithoutBrackets,
    /// A `path` attribute.
    PathAttribute,
    /// The name `cfg_select`.
    Select,
    /// The name `tt`.
    Tt,
    /// A `cfg` inside a `cfg_attr`.
    InCfgAttr,
    /// A call of `include!`.
    Include,
}

impl fmt::Display for CfgFix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CfgFix::Predicate { form } => write!(
                f,
                "remove {form} from this cfg; the cfgs of a crate whose row bans I/O use only \
             `test`, `doc` and features, joined by `all`, `any` and `not`, with no feature \
             under a `not`"
            ),
            CfgFix::MetavariableAttribute => f.write_str(
                "write the attribute's name in the macro body itself; an outer attribute may \
             instead come whole from the caller, bound as `#[$m:meta]` in the arm's matcher \
             and written as `#[$m]`, in a `macro_rules!` outside every other macro's body \
             and arguments",
            ),
            CfgFix::MetavariableMacro => {
                f.write_str("write the called macro's name in the macro body itself")
            }
            CfgFix::HashWithoutBrackets => f.write_str(
                "write the attribute's brackets right after its `#`, as `#[..]` or `#![..]`; to \
             pass an attribute to a macro, pass it whole, with its `#` and its brackets",
            ),
            CfgFix::PathAttribute => f.write_str(
                "give `#[path = \"..\"]` only to an out-of-line `mod name;` at the top level of a \
             file, outside every brace, bracket and parenthesis, with a string literal that \
             names, relative to this file's folder and with `/` between components, a `.rs` \
             file under the crate's folder that git lists and that is not a symlink, without \
             leaving that folder on the way; a `path` in a `cfg_attr`, on an inline module, \
             inside a module block or with a metavariable value always fails",
            ),
            CfgFix::Select => f.write_str(
                "rename a field, variable or import named `cfg_select`; for the macro, give each \
             arm's code its own `#[cfg(..)]` built from `test`, `doc` and positive features, \
             or use `if cfg!(..)`, which compiles both branches",
            ),
            CfgFix::Tt => f.write_str(
                "give the macros of a crate whose row bans I/O no `tt` fragment and use a typed \
             fragment such as `item`, `expr`, `ident`, `ty` or `meta`; rename an identifier \
             named `tt`",
            ),
            CfgFix::InCfgAttr => f.write_str(
                "write the condition as one `#[cfg(..)]` outside any `cfg_attr`, built from \
             `test`, `doc` and positive features",
            ),
            CfgFix::Include => f.write_str(
                "move the code into a module at its default path; use `include_str!` or \
             `include_bytes!` for data",
            ),
        }
    }
}
