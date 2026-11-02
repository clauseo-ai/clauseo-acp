//! The child's environment: inherited by default, only the spec's with `SpawnOptions::clear_env`.
//! `/usr/bin/env` prints it, so these run on Unix only. The helpers carry `#[cfg(test)]` so clippy
//! treats them as test code.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsString;

use async_io::block_on;
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use futures::{AsyncReadExt, join};

/// Runs `/usr/bin/env` with `CLAUSEO_PROBE=1` through a passthrough launcher with `opts` and
/// returns the lines it prints, sorted.
#[cfg(test)]
fn env_lines(opts: SpawnOptions) -> Vec<String> {
    let env = BTreeMap::from([(OsString::from("CLAUSEO_PROBE"), OsString::from("1"))]);
    let spec = LaunchSpec::new(
        "/usr/bin/env".into(),
        Vec::new(),
        env,
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap();
    let Launched {
        stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = PassthroughLauncher::new(opts).launch(&spec).unwrap();
    drop(stdin);
    let (out, report) = block_on(async {
        let mut out = Vec::new();
        let (read, pumped, report) = join!(
            stdout.read_to_end(&mut out),
            stderr.drain(),
            handle.exited()
        );
        read.unwrap();
        pumped.unwrap();
        (out, report)
    });
    assert_eq!(report.code(), Some(0));
    let mut lines: Vec<String> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}

/// One variable of this test process's environment (the test runner sets several), as the
/// `NAME=value` line `env` prints for it.
#[cfg(test)]
fn inherited_line() -> String {
    std::env::vars()
        .find(|(name, value)| name != "CLAUSEO_PROBE" && !value.contains('\n'))
        .map(|(name, value)| format!("{name}={value}"))
        .expect("the test process has an environment")
}

/// With default options the child inherits this process's variables and gets the spec's.
#[test]
fn default_options_inherit_the_environment_and_add_the_specs() {
    let inherited = inherited_line();
    let lines = env_lines(SpawnOptions::new());
    assert!(lines.contains(&"CLAUSEO_PROBE=1".to_owned()), "{lines:?}");
    assert!(lines.contains(&inherited), "{lines:?}");
}

/// With `clear_env` the child gets the spec's variables and nothing else.
#[test]
fn clear_env_gives_the_child_only_the_specs_variables() {
    let lines = env_lines(SpawnOptions::new().clear_env());
    assert_eq!(lines, ["CLAUSEO_PROBE=1"]);
}
