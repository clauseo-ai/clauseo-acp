//! The violations of the cfg rule's findings: each message with its fix kind, in `fix.rs`.

use crate::checks::core_io::cfgs::Finding;
use crate::checks::core_io::fix::Fix;
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
            Fix::CfgPredicate { form: form.clone() },
        ),
        Finding::MetavariableAttribute => (
            format!(
                "`{path}` writes an attribute whose name is a metavariable, so a macro of \
                 {package} can build a cfg that core-io does not see"
            ),
            Fix::CfgMetavariableAttribute,
        ),
        Finding::MetavariableMacro => (
            format!(
                "`{path}` calls a macro whose name is a metavariable, so a macro of {package} can \
                 call `include!` under a name passed to it"
            ),
            Fix::CfgMetavariableMacro,
        ),
        Finding::HashWithoutBrackets => (
            format!(
                "`{path}` writes a `#` that is followed by neither brackets nor `!` and \
                 brackets, so a macro can join it to brackets from elsewhere and build an \
                 attribute in {package} that core-io does not see"
            ),
            Fix::CfgHashWithoutBrackets,
        ),
        Finding::PathAttribute => (
            format!(
                "`{path}` gives a module a `path` attribute that core-io cannot follow to a file \
                 it lexes, so it can bring into {package} a file core-io does not read"
            ),
            Fix::CfgPathAttribute,
        ),
        Finding::CfgSelect => (
            format!(
                "`{path}` names `cfg_select`, a name a crate whose row bans I/O never uses, since \
                 `cfg_select!` builds only the first arm whose cfg holds and a `use` can call it \
                 under another name"
            ),
            Fix::CfgSelect,
        ),
        Finding::TtFragment => (
            format!(
                "`{path}` names `tt`, a name a crate whose row bans I/O never uses, since a \
                 `tt` fragment can take a lone `#` out of an attribute and a macro of {package} \
                 can join it to other brackets into an attribute that core-io does not see"
            ),
            Fix::CfgTt,
        ),
        Finding::CfgInCfgAttr => (
            format!(
                "`{path}` puts a `cfg` inside a `cfg_attr`, which keeps the item whenever the \
                 `cfg_attr`'s predicate is false, so a feature there acts like a `not(feature)` \
                 and can leave code out of every clippy run"
            ),
            Fix::CfgInCfgAttr,
        ),
        Finding::Include => (
            format!(
                "`{path}` calls `include!`, which can bring into {package} code from a file \
                 core-io does not read"
            ),
            Fix::CfgInclude,
        ),
    };
    violation(path, line, message, fix)
}
