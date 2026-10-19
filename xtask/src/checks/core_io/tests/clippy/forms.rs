//! The forms `clippy_bans_every_planted_call` builds, one module each. A must-fail form gets at
//! least one `disallowed_methods` or `disallowed_types` hit inside its own file; a must-pass form
//! gets none. To add one, push it in `forms()` (or `review::forms()`): a name that is a valid
//! module name, whether it must fail or pass, the targets it builds on, and its source.

mod cases;
mod review;
mod rules;

/// What clippy must report for a form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Must {
    /// At least one hit in the form's file.
    Fail,
    /// No hit in the form's file.
    Pass,
}

/// The targets a form builds on: a form that names `std::os::unix` does not build on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Only {
    Any,
    Unix,
    Windows,
}

impl Only {
    /// True when the form builds on the target the test runs on.
    pub(super) fn here(self) -> bool {
        match self {
            Only::Any => true,
            Only::Unix => cfg!(unix),
            Only::Windows => cfg!(windows),
        }
    }
}

/// One module of the scratch crate.
#[derive(Debug, Clone)]
pub(super) struct Form {
    pub name: String,
    pub must: Must,
    pub only: Only,
    pub source: String,
}

/// A form that must fail on every target.
pub(super) fn fail(name: &str, source: &str) -> Form {
    form(name, Must::Fail, Only::Any, source)
}

/// A form that must pass on every target.
pub(super) fn pass(name: &str, source: &str) -> Form {
    form(name, Must::Pass, Only::Any, source)
}

pub(super) fn form(name: &str, must: Must, only: Only, source: &str) -> Form {
    Form {
        name: name.to_owned(),
        must,
        only,
        source: source.to_owned(),
    }
}

/// Every form: the module and clock shapes of the old syn check's planted cases, its other
/// cases and `macro_rules!` cases, then the review's (`review.rs`).
pub(super) fn forms() -> Vec<Form> {
    let mut all = Vec::new();
    for banned in [FS, NET, PROCESS, ENV] {
        for shape in SHAPES {
            let name = format!("{}_{}", banned.module, shape_name(shape));
            all.push(fail(&name, &module_source(&banned, shape)));
        }
    }
    for clock in ["Instant", "SystemTime"] {
        for shape in CLOCK_SHAPES {
            let name = format!("{}_{}", clock.to_lowercase(), clock_shape_name(shape));
            all.push(fail(&name, &clock_source(clock, shape)));
        }
    }
    all.extend(cases::failing());
    all.extend(cases::passing());
    all.extend(rules::failing());
    all.extend(rules::passing());
    all.extend(review::forms());
    all
}

/// A banned module and its items, for the module shapes.
struct Banned {
    /// `fs`.
    module: &'static str,
    /// An item to import: `read`.
    item: &'static str,
    /// A call that starts with `item`, as written after `std::<module>::`.
    call: &'static str,
    /// A function named as a value, after `std::<module>::`.
    value: &'static str,
}

const FS: Banned = Banned {
    module: "fs",
    item: "read",
    call: "read(\"a\")",
    value: "read::<&str>",
};
const NET: Banned = Banned {
    module: "net",
    item: "TcpStream",
    call: "TcpStream::connect(\"a:1\")",
    value: "TcpStream::connect::<&str>",
};
const PROCESS: Banned = Banned {
    module: "process",
    item: "Command",
    call: "Command::new(\"a\")",
    value: "Command::new::<&str>",
};
const ENV: Banned = Banned {
    module: "env",
    item: "var",
    call: "var(\"a\")",
    value: "var::<&str>",
};

/// The ways a module form reaches the banned module; each ends in a use, since an import of a
/// module or a function alone is no hit.
#[derive(Clone, Copy)]
enum Shape {
    Use,
    Group,
    Nested,
    StdAlias,
    LeadingColon,
    Qualified,
    ExternCrate,
    FnBody,
    InlineMod,
    Glob,
    StdGlob,
    Macro,
    SelfAlias,
    Rename,
    Value,
    Raw,
}

const SHAPES: [Shape; 16] = [
    Shape::Use,
    Shape::Group,
    Shape::Nested,
    Shape::StdAlias,
    Shape::LeadingColon,
    Shape::Qualified,
    Shape::ExternCrate,
    Shape::FnBody,
    Shape::InlineMod,
    Shape::Glob,
    Shape::StdGlob,
    Shape::Macro,
    Shape::SelfAlias,
    Shape::Rename,
    Shape::Value,
    Shape::Raw,
];

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Use => "use",
        Shape::Group => "group",
        Shape::Nested => "nested",
        Shape::StdAlias => "std_alias",
        Shape::LeadingColon => "leading_colon",
        Shape::Qualified => "qualified",
        Shape::ExternCrate => "extern_crate",
        Shape::FnBody => "fn_body",
        Shape::InlineMod => "inline_mod",
        Shape::Glob => "glob",
        Shape::StdGlob => "std_glob",
        Shape::Macro => "macro",
        Shape::SelfAlias => "self_alias",
        Shape::Rename => "rename",
        Shape::Value => "value",
        Shape::Raw => "raw",
    }
}

/// The source of one module form.
fn module_source(banned: &Banned, shape: Shape) -> String {
    let Banned {
        module: m,
        item,
        call,
        value,
    } = banned;
    let body = |line: String| format!("pub fn f() {{\n    {line}\n}}\n");
    match shape {
        Shape::Use => format!(
            "use std::{m}::{item};\n{}",
            body(format!("let _ = {call};"))
        ),
        Shape::Group => format!(
            "use std::{{{m}, io}};\n{}",
            body(format!("let _ = {m}::{call};"))
        ),
        Shape::Nested => format!(
            "use std::{{io, {{{m}::{item}}}}};\n{}",
            body(format!("let _ = {call};"))
        ),
        Shape::StdAlias => format!(
            "use std as s;\n{}",
            body(format!("let _ = s::{m}::{call};"))
        ),
        Shape::LeadingColon => body(format!("let _ = ::std::{m}::{call};")),
        Shape::Qualified => body(format!("let _ = std::{m}::{call};")),
        Shape::ExternCrate => format!(
            "extern crate std as x;\n{}",
            body(format!("let _ = x::{m}::{call};"))
        ),
        Shape::FnBody => body(format!("use std::{m};\n    let _ = {m}::{call};")),
        Shape::InlineMod => format!(
            "mod inner {{\n    pub use std::{m}::{item};\n}}\n{}",
            body(format!("let _ = inner::{call};"))
        ),
        Shape::Glob => format!("use std::{m}::*;\n{}", body(format!("let _ = {call};"))),
        Shape::StdGlob => format!("use std::*;\n{}", body(format!("let _ = {m}::{call};"))),
        Shape::Macro => body(format!("let _ = vec![std::{m}::{call}];")),
        Shape::SelfAlias => format!(
            "use std::{{self as t}};\n{}",
            body(format!("let _ = t::{m}::{call};"))
        ),
        Shape::Rename => format!(
            "use std::{m} as f;\n{}",
            body(format!("let _ = f::{call};"))
        ),
        Shape::Value => body(format!("let g = std::{m}::{value};\n    let _ = g;")),
        Shape::Raw => body(format!("let _ = std::r#{m}::{call};")),
    }
}

/// The ways a clock form reaches `<clock>::now`.
#[derive(Clone, Copy)]
enum ClockShape {
    Use,
    Rename,
    TypeAlias,
    Qualified,
    Glob,
    SelfInImpl,
    ParenAlias,
}

const CLOCK_SHAPES: [ClockShape; 7] = [
    ClockShape::Use,
    ClockShape::Rename,
    ClockShape::TypeAlias,
    ClockShape::Qualified,
    ClockShape::Glob,
    ClockShape::SelfInImpl,
    ClockShape::ParenAlias,
];

fn clock_shape_name(shape: ClockShape) -> &'static str {
    match shape {
        ClockShape::Use => "use",
        ClockShape::Rename => "rename",
        ClockShape::TypeAlias => "type_alias",
        ClockShape::Qualified => "qualified",
        ClockShape::Glob => "glob",
        ClockShape::SelfInImpl => "self_in_impl",
        ClockShape::ParenAlias => "paren_alias",
    }
}

/// The source of one clock form.
fn clock_source(clock: &str, shape: ClockShape) -> String {
    let call = |line: &str| format!("pub fn f() {{\n    let _ = {line};\n}}\n");
    match shape {
        ClockShape::Use => format!(
            "use std::time::{clock};\n{}",
            call(&format!("{clock}::now()"))
        ),
        ClockShape::Rename => format!("use std::time::{clock} as I;\n{}", call("I::now()")),
        ClockShape::TypeAlias => format!("type T = std::time::{clock};\n{}", call("T::now()")),
        ClockShape::Qualified => call(&format!("<std::time::{clock}>::now()")),
        ClockShape::Glob => format!("use std::time::*;\n{}", call(&format!("{clock}::now()"))),
        ClockShape::SelfInImpl => format!(
            "pub trait N {{\n    fn n() -> Self;\n}}\nimpl N for std::time::{clock} {{\n    \
             fn n() -> Self {{\n        Self::now()\n    }}\n}}\n"
        ),
        ClockShape::ParenAlias => format!("type T = (std::time::{clock});\n{}", call("T::now()")),
    }
}
