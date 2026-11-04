//! The host's launcher, seen from a child it starts: `/usr/bin/env` on Unix and `cmd /c set`
//! on Windows print the child's environment.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_process::{LaunchSpec, Launched, Launcher as _};
use futures::{AsyncReadExt as _, join};

/// The lines the child `program args` prints, started by the host's launcher with `env` and
/// stdin closed, sorted. The child must exit 0.
fn child_lines(program: PathBuf, args: &[&str], env: BTreeMap<OsString, OsString>) -> Vec<String> {
    let args = args.iter().map(OsString::from).collect();
    let spec = LaunchSpec::new(program, args, env, env!("CARGO_MANIFEST_DIR").into()).unwrap();
    let Launched {
        stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = crate::host_launcher(None).launch(&spec).unwrap();
    drop(stdin);
    let (out, report) = async_io::block_on(async {
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

/// The launcher the host gets clears the environment: a child sees its spec's variables and
/// none of this process's (which holds the test runner's `PATH`, `HOME` and `CARGO_*`).
#[cfg(unix)]
#[test]
fn the_host_launcher_gives_the_child_only_its_specs_variables() {
    assert!(
        std::env::var_os("HOME").is_some(),
        "the test runner sets HOME"
    );
    let env = BTreeMap::from([
        (OsString::from("CLAUSEO_PROBE"), OsString::from("1")),
        (OsString::from("HOME"), OsString::from("/home/probe")),
        (OsString::from("PATH"), OsString::from("/probe/bin")),
    ]);
    let lines = child_lines("/usr/bin/env".into(), &[], env);
    assert_eq!(
        lines,
        ["CLAUSEO_PROBE=1", "HOME=/home/probe", "PATH=/probe/bin"]
    );
}

/// On Windows the child gets the spec's `SystemRoot` (Winsock and Node need it) and not this
/// process's `USERPROFILE`, which the spec leaves out here.
#[cfg(windows)]
#[test]
fn the_host_launcher_gives_the_child_only_its_specs_variables() {
    let system_root = std::env::var_os("SystemRoot").expect("Windows sets SystemRoot");
    assert!(
        std::env::var_os("USERPROFILE").is_some(),
        "Windows sets USERPROFILE"
    );
    let cmd = PathBuf::from(&system_root).join("System32").join("cmd.exe");
    let env = BTreeMap::from([
        (OsString::from("CLAUSEO_PROBE"), OsString::from("1")),
        (OsString::from("SystemRoot"), system_root.clone()),
    ]);
    let lines = child_lines(cmd, &["/d", "/c", "set"], env);
    let root = format!("SystemRoot={}", system_root.to_string_lossy());
    assert!(lines.contains(&root), "{lines:?}");
    assert!(lines.contains(&"CLAUSEO_PROBE=1".to_owned()), "{lines:?}");
    let profile = lines
        .iter()
        .find(|line| line.to_ascii_uppercase().starts_with("USERPROFILE="));
    assert_eq!(profile, None);
}
