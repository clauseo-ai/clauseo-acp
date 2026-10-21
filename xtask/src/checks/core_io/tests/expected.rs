//! The expected lines of the core-io planted cases that more than one case prints, as macros the
//! cases in `planted.rs` use. Each expands to one `fails` line, or to the `Fix` of a violation
//! that several cases share. The location and the message are spelled out here, byte for byte as
//! the check prints them; the fix is the check's `Fix` variant, so its text lives in `fix.rs`.

/// The text of `crates/protocol/clippy.toml` the fix prints: the root clippy.toml with the list's
/// arrays appended, without a newline after its last line.
#[cfg(test)]
macro_rules! crate_file {
    () => {
        "# Generated: the root clippy.toml with the arrays of xtask/core-io-bans.toml appended, root\n\
         # entries first, since clippy reads only the nearest clippy.toml. `cargo xtask check`\n\
         # (core-io) fails when this file parses to anything else and prints the file to write.\n\
         msrv = \"1.99\"\n\
         disallowed-methods = [\n  \
         { path = \"std::time::Instant::now\", reason = \"the core takes time as input\" },\n  \
         { path = \"std::fs::read\", reason = \"no files\" },\n\
         ]\n\
         disallowed-types = [\n  \
         { allow-invalid = true, path = \"std::fs::File\", reason = \"no files\" },\n\
         ]"
    };
}

/// The fix of every violation about `crates/protocol/clippy.toml`: write the file as
/// `crate_file!` gives it.
#[cfg(test)]
macro_rules! write_crate_file {
    () => {
        $crate::checks::core_io::Fix::WriteCrateFile {
            path: "crates/protocol/clippy.toml".to_owned(),
            package: "clauseo-protocol".to_owned(),
            text: crate_file!().to_owned(),
        }
    };
}

/// The violation for the stray clippy config `$path`, a `clippy.toml` in `$dir`.
#[cfg(test)]
macro_rules! stray {
    ($path:literal, $dir:literal) => {
        $crate::checks::planted::fails(
            concat!($path, ":1"),
            concat!(
                "`", $path, "` is a clippy.toml other than the root one and those core-io \
                 writes for the crates whose row bans fs, process or any I/O; clippy reads only \
                 the nearest one, so it replaces the root clippy.toml's bans for the crates under \
                 `", $dir, "/`"
            ),
            $crate::checks::core_io::Fix::StrayConfig {
                path: $path.to_owned(),
            },
        )
    };
}

/// The violation for the target root `$path` of `clauseo-protocol`, a `$kind` target, without
/// the forbid line.
#[cfg(test)]
macro_rules! no_forbid {
    ($path:literal, $kind:literal) => {
        $crate::checks::planted::fails(
            concat!($path, ":1"),
            concat!(
                "`", $path, "`, the root of clauseo-protocol's ", $kind, " target, does not carry \
                 `#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]` outside any \
                 `cfg_attr`, so an `allow` or `expect` in the target can turn off the bans of \
                 clippy.toml and xtask/core-io-bans.toml"
            ),
            $crate::checks::core_io::Fix::AddForbid {
                path: $path.to_owned(),
            },
        )
    };
}

/// The violation for `crates/protocol/Cargo.toml`, which gives a `$kind` target the root
/// `$path`, outside the crate's folder.
#[cfg(test)]
macro_rules! root_outside {
    ($kind:literal, $path:literal) => {
        $crate::checks::planted::fails(
            "crates/protocol/Cargo.toml:1",
            concat!(
                "`crates/protocol/Cargo.toml` gives clauseo-protocol's ",
                $kind,
                " target the root \
                 `",
                $path,
                "`, outside `crates/protocol/`, and core-io checks the forbid line and \
                 the cfgs of clauseo-protocol only under its folder"
            ),
            $crate::checks::core_io::Fix::MoveRoot {
                manifest: "crates/protocol/Cargo.toml".to_owned(),
                path: $path.to_owned(),
                dir: "crates/protocol".to_owned(),
            },
        )
    };
}

/// The violation for the `[alias]` key `$name` of `.cargo/config.toml` at `$line`.
#[cfg(test)]
macro_rules! alias {
    ($line:literal, $name:literal) => {
        $crate::checks::planted::fails(
            concat!(".cargo/config.toml:", $line),
            concat!(
                "`.cargo/config.toml` defines the alias `", $name, "`, so `cargo ", $name, "` runs \
                 the alias instead of the external command xtask runs, and cargo only warns about \
                 it"
            ),
            $crate::checks::core_io::Fix::RemoveAlias {
                name: $name.to_owned(),
            },
        )
    };
}

/// The violation for the `rustflags` key `$key` of `.cargo/config.toml` at `$line`.
#[cfg(test)]
macro_rules! rustflags {
    ($line:literal, $key:literal) => {
        $crate::checks::planted::fails(
            concat!(".cargo/config.toml:", $line),
            concat!(
                "`.cargo/config.toml` sets `", $key, "`, where `-A`, `--cap-lints` or `-A \
                 clippy::style` would silence the bans of clippy.toml and xtask/core-io-bans.toml \
                 for every crate; lint levels live only in the root [workspace.lints]"
            ),
            $crate::checks::core_io::Fix::RemoveRustflags,
        )
    };
}

/// The violation for the compiler key `$key` of `.cargo/config.toml` at `$line`.
#[cfg(test)]
macro_rules! compiler {
    ($line:literal, $key:literal) => {
        $crate::checks::planted::fails(
            concat!(".cargo/config.toml:", $line),
            concat!(
                "`.cargo/config.toml` sets `",
                $key,
                "`, which replaces or wraps the compiler \
                 clippy runs, so it can add `--cap-lints allow` and turn off the bans of \
                 clippy.toml and xtask/core-io-bans.toml past the forbid line"
            ),
            $crate::checks::core_io::Fix::RemoveCompiler {
                key: $key.to_owned(),
            },
        )
    };
}

/// The violation for the `[env]` entry `$name` of `.cargo/config.toml` at `$line`.
#[cfg(test)]
macro_rules! clippy_env {
    ($line:literal, $name:literal) => {
        $crate::checks::planted::fails(
            concat!(".cargo/config.toml:", $line),
            concat!(
                "`.cargo/config.toml` sets `",
                $name,
                "` in [env], which cargo passes to every \
                 clippy-driver it starts; `CLIPPY_CONF_DIR` there points clippy away from the \
                 clippy.toml files core-io checks"
            ),
            $crate::checks::core_io::Fix::RemoveClippyEnv,
        )
    };
}

/// The violation for a member's manifest `$path`, at `$line`, with its own `[lints]` table.
#[cfg(test)]
macro_rules! own_lints {
    ($path:literal, $line:literal, $package:literal) => {
        $crate::checks::planted::fails(
            concat!($path, ":", $line),
            concat!(
                "`", $path, "` sets ", $package, "'s own lints instead of `[lints] workspace = \
                 true`; lint levels live only in the root [workspace.lints], so that no crate can \
                 turn off the bans of the root clippy.toml or xtask/core-io-bans.toml"
            ),
            $crate::checks::core_io::Fix::ReplaceLintsTable,
        )
    };
}

/// The violation for a member's manifest `$path` with no `[lints]` table.
#[cfg(test)]
macro_rules! no_lints {
    ($path:literal, $package:literal) => {
        $crate::checks::planted::fails(
            concat!($path, ":1"),
            concat!(
                "`",
                $path,
                "` has no `[lints] workspace = true`, so ",
                $package,
                " does not get \
                 the lint levels of the root [workspace.lints], the one place they are set"
            ),
            $crate::checks::core_io::Fix::AddLints,
        )
    };
}

/// The violation for a cfg with `$form` at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! cfg_form {
    ($line:literal, $form:literal) => {
        $crate::checks::planted::fails(
            concat!("crates/protocol/src/hidden.rs:", $line),
            concat!(
                "`crates/protocol/src/hidden.rs` has a cfg with ", $form, "; clippy lints \
                 clauseo-protocol once per target kind with every feature on, so only a cfg built \
                 from `test` and positive features holds in one of its runs, `doc` holds only when \
                 rustdoc documents the crate, and code behind any other cfg can be left out of \
                 every clippy run"
            ),
            $crate::checks::core_io::Fix::Cfg(
                $crate::checks::core_io::fix::CfgFix::Predicate {
                    form: $form.to_owned(),
                },
            ),
        )
    };
}

/// The violation for a `path` attribute at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! path_attribute {
    ($line:literal) => {
        $crate::checks::planted::fails(
            concat!("crates/protocol/src/hidden.rs:", $line),
            "`crates/protocol/src/hidden.rs` gives a module a `path` attribute that core-io cannot \
             follow to a file it lexes, so it can bring into clauseo-protocol a file core-io does \
             not read",
            $crate::checks::core_io::Fix::Cfg(
                $crate::checks::core_io::fix::CfgFix::PathAttribute,
            ),
        )
    };
}

/// The violation for an attribute whose name is a metavariable at line `$line` of
/// `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! metavariable_attribute {
    ($line:literal) => {
        $crate::checks::planted::fails(
            concat!("crates/protocol/src/hidden.rs:", $line),
            "`crates/protocol/src/hidden.rs` writes an attribute whose name is a metavariable, \
             so a macro of clauseo-protocol can build a cfg that core-io does not see",
            $crate::checks::core_io::Fix::Cfg(
                $crate::checks::core_io::fix::CfgFix::MetavariableAttribute,
            ),
        )
    };
}

/// The violation for a `#` without brackets at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! hash_without_brackets {
    ($line:literal) => {
        $crate::checks::planted::fails(
            concat!("crates/protocol/src/hidden.rs:", $line),
            "`crates/protocol/src/hidden.rs` writes a `#` that is followed by neither brackets nor \
             `!` and brackets, so a macro can join it to brackets from elsewhere and build an \
             attribute in clauseo-protocol that core-io does not see",
            $crate::checks::core_io::Fix::Cfg(
                $crate::checks::core_io::fix::CfgFix::HashWithoutBrackets,
            ),
        )
    };
}

/// The violation for the name `cfg_select` at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! cfg_select {
    ($line:literal) => {
        $crate::checks::planted::fails(
            concat!("crates/protocol/src/hidden.rs:", $line),
            "`crates/protocol/src/hidden.rs` names `cfg_select`, a name a crate whose row bans I/O \
             never uses, since `cfg_select!` builds only the first arm whose cfg holds and a `use` \
             can call it under another name",
            $crate::checks::core_io::Fix::Cfg($crate::checks::core_io::fix::CfgFix::Select),
        )
    };
}

/// The violation for the name `tt` at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! tt_fragment {
    ($line:literal) => {
        $crate::checks::planted::fails(
            concat!("crates/protocol/src/hidden.rs:", $line),
            "`crates/protocol/src/hidden.rs` names `tt`, a name a crate whose row bans I/O never \
             uses, since a `tt` fragment can take a lone `#` out of an attribute and a macro of \
             clauseo-protocol can join it to other brackets into an attribute that core-io does \
             not see",
            $crate::checks::core_io::Fix::Cfg($crate::checks::core_io::fix::CfgFix::Tt),
        )
    };
}
