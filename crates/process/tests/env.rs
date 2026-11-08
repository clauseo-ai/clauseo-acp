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
    env_lines_with(opts, &[])
}

/// [`env_lines`], with `extra` in the spec too.
#[cfg(test)]
fn env_lines_with(opts: SpawnOptions, extra: &[(&str, &str)]) -> Vec<String> {
    let mut env = BTreeMap::from([(OsString::from("CLAUSEO_PROBE"), OsString::from("1"))]);
    env.extend(
        extra
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value))),
    );
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

/// Set on the run of this binary that [`inherited_node_variables_reach_no_child`] starts.
const NODE_ENV_CHILD: &str = "CLAUSEO_NODE_ENV_CHILD";

/// `NODE_OPTIONS` and `NODE_PATH` in a spec reach no child, with either options.
#[test]
fn node_variables_in_the_spec_reach_no_child() {
    let node = [("NODE_OPTIONS", "--require=/x.js"), ("NODE_PATH", "/x")];
    for opts in [SpawnOptions::new(), SpawnOptions::new().clear_env()] {
        let lines = env_lines_with(opts, &node);
        assert!(lines.contains(&"CLAUSEO_PROBE=1".to_owned()), "{lines:?}");
        assert!(
            !lines.iter().any(|line| line.starts_with("NODE_")),
            "{lines:?}"
        );
    }
}

/// `NODE_OPTIONS` and `NODE_PATH` in this process's environment reach no child that inherits
/// it. This test's environment is left alone: it runs this binary again with both set, and
/// that run's [`node_variables_of_the_parent_stay_out`] does the check.
#[test]
fn inherited_node_variables_reach_no_child() {
    let exe = std::env::current_exe().unwrap();
    let out = block_on(
        async_process::Command::new(exe)
            .args([
                "--exact",
                "node_variables_of_the_parent_stay_out",
                "--nocapture",
            ])
            .env(NODE_ENV_CHILD, "1")
            .env("NODE_OPTIONS", "--require=/x.js")
            .env("NODE_PATH", "/x")
            .output(),
    )
    .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("1 passed"), "{text}");
}

/// The check [`inherited_node_variables_reach_no_child`] runs; on its own it does nothing.
#[test]
fn node_variables_of_the_parent_stay_out() {
    if std::env::var_os(NODE_ENV_CHILD).is_none() {
        return;
    }
    assert!(std::env::var_os("NODE_OPTIONS").is_some());
    let lines = env_lines(SpawnOptions::new());
    assert!(lines.contains(&format!("{NODE_ENV_CHILD}=1")), "{lines:?}");
    assert!(
        !lines.iter().any(|line| line.starts_with("NODE_")),
        "{lines:?}"
    );
}
