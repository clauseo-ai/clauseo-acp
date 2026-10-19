//! The old syn check's cases with sources of their own that are Rust code a crate compiles. Its
//! cases about `#[path]`, `include!`, symlinks and crate roots were about what syn could not
//! read; clippy reads what the compiler builds.

use super::{Form, Must, Only, fail, form, pass};

/// The cases that must fail.
pub(super) fn failing() -> Vec<Form> {
    let instant_assoc = "pub trait K {\n    type C;\n    fn t();\n}\npub struct Z;\n\
                         impl K for Z {\n    type C = std::time::Instant;\n";
    vec![
        fail(
            "raw_std",
            "pub fn f() {\n    let _ = r#std::fs::read(\"a\");\n}\n",
        ),
        fail(
            "raw_use",
            "use std::r#fs;\npub fn f() {\n    let _ = fs::read(\"a\");\n}\n",
        ),
        fail(
            "instant_raw_now",
            "use std::time::Instant;\npub fn f() {\n    let _ = Instant::r#now();\n}\n",
        ),
        fail(
            "associated_type_qualified",
            &format!(
                "{instant_assoc}    fn t() {{}}\n}}\npub fn f() {{\n    let _ = <Z as K>::C::now();\n}}\n"
            ),
        ),
        fail(
            "associated_type_self",
            &format!("{instant_assoc}    fn t() {{\n        let _ = Self::C::now();\n    }}\n}}\n"),
        ),
        form(
            "os_unix_fs",
            Must::Fail,
            Only::Unix,
            "pub fn f() {\n    let _ = std::os::unix::fs::symlink(\"a\", \"b\");\n}\n",
        ),
        form(
            "os_unix_net",
            Must::Fail,
            Only::Unix,
            "use std::os::unix::net::UnixStream;\npub fn f() {\n    \
             let _ = UnixStream::connect(\"a\");\n}\n",
        ),
        form(
            "os_unix_process",
            Must::Fail,
            Only::Unix,
            "use std::os::unix::process::CommandExt;\n\
             pub fn f<C: CommandExt>(c: &mut C) {\n    c.uid(1);\n}\n",
        ),
        form(
            "os_windows_fs",
            Must::Fail,
            Only::Windows,
            "pub fn f() {\n    let _ = std::os::windows::fs::symlink_file(\"a\", \"b\");\n}\n",
        ),
        form(
            "os_glob",
            Must::Fail,
            Only::Unix,
            "use std::os::*;\npub fn f() {\n    let _ = unix::fs::symlink(\"a\", \"b\");\n}\n",
        ),
        form(
            "os_unix_glob",
            Must::Fail,
            Only::Unix,
            "use std::os::unix::*;\npub fn f() {\n    let _ = fs::symlink(\"a\", \"b\");\n}\n",
        ),
        form(
            "os_prelude_glob",
            Must::Fail,
            Only::Unix,
            "use std::os::unix::prelude::*;\npub fn f<P: PermissionsExt>(p: &P) -> u32 {\n    \
             p.mode()\n}\n",
        ),
        fail(
            "macro_block_use",
            "pub fn f() {\n    let _ = vec![{\n        use std::time::Instant as I;\n        \
             I::now()\n    }];\n}\n",
        ),
        fail(
            "macro_rules_item",
            "macro_rules! a {\n    () => {\n        type U = std::time::Instant;\n    };\n}\n\
             a!();\npub fn f() {\n    let _ = U::now();\n}\n",
        ),
        fail(
            "extern_crate_self",
            "pub use std as s;\npub fn f() {\n    let _ = crate::extern_crate_self::s::fs::read(\"a\");\n}\n",
        ),
        fail(
            "use_then_call",
            "use std::fs;\npub fn f() {\n    let _ = fs::read(\"a\");\n}\n",
        ),
        fail(
            "alias_in_another_module",
            "mod clock {\n    pub type Clock = std::time::Instant;\n}\n\
             pub fn f() {\n    let _ = clock::Clock::now();\n}\n",
        ),
        fail(
            "std_reexported",
            "pub use std as s;\nmod a {\n    pub fn f() {\n        let _ = super::s::fs::read(\"a\");\n    }\n}\n",
        ),
        fail(
            "associated_type_in_a_bound",
            "pub trait K {\n    type C;\n}\npub fn f<T: K<C = std::time::Instant>>() {\n    \
             let _ = <T::C>::now();\n}\n",
        ),
    ]
}

/// The cases that must pass.
pub(super) fn passing() -> Vec<Form> {
    let local_now = "pub struct L;\nimpl L {\n    pub fn now() -> L {\n        L\n    }\n}\n";
    vec![
        pass(
            "allowed_duration",
            "use std::time::Duration;\npub fn f(d: Duration) -> Duration {\n    \
             d + std::time::Duration::from_secs(1)\n}\n",
        ),
        pass(
            "allowed_io_error",
            "use std::io::Error;\npub fn f(d: u8) -> Error {\n    Error::other(format!(\"{d}\"))\n}\n",
        ),
        pass(
            "instant_as_a_type",
            "use std::time::Instant;\n\
             pub fn f(start: Instant, end: std::time::Instant) -> std::time::Duration {\n    \
             end - start\n}\n",
        ),
        pass(
            "local_names_like_banned_ones",
            "mod fs {\n    pub fn read(_: &str) {}\n}\nmod other {\n    pub mod time {\n        \
             pub struct Instant;\n        impl Instant {\n            pub fn now() {}\n        }\n    \
             }\n}\npub fn f() {\n    fs::read(\"a\");\n    other::time::Instant::now();\n}\n",
        ),
        pass(
            "core_net",
            "pub fn f() -> core::net::Ipv4Addr {\n    core::net::Ipv4Addr::LOCALHOST\n}\n",
        ),
        pass(
            "std_env_macro",
            "pub fn f() -> &'static str {\n    std::env!(\"CARGO_PKG_NAME\")\n}\n",
        ),
        pass(
            "associated_name_like_a_local_type",
            "pub trait K {\n    type Output;\n}\npub struct Z;\nimpl K for Z {\n    \
             type Output = std::time::Instant;\n}\npub struct Output;\nimpl Output {\n    \
             pub fn now() -> Output {\n        Output\n    }\n}\npub fn f() {\n    \
             let _ = Output::now();\n}\n",
        ),
        pass(
            "associated_type_bound_in_scope",
            &format!(
                "{local_now}pub trait K {{\n    type C;\n}}\npub trait K2 {{\n    type C;\n}}\n\
                 pub fn a<T: K<C = std::time::Instant>>() {{}}\n\
                 pub fn b<U: K2<C = L>>() {{\n    let _ = U::C::now();\n}}\n"
            ),
        ),
    ]
}
