//! The Keychain query's run (`super::claude_keychain`): any program and arguments, with
//! their output thrown away and only the exit status kept. It names no credential; the
//! arguments, attributes only, are `super::keychain_args`.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_process::{Launcher, Timer};

use super::KeychainProbe;

/// Runs `program args` through `launcher` with only `HOME` from `env` (where the Security
/// framework finds the user's keychains), stdin closed at once, stdout and stderr read to their
/// end and dropped (never logged: they name the account), killed past `limit` (the handle's
/// drop kills its tree).
pub(crate) async fn run<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    program: &Path,
    args: Vec<OsString>,
    env: &BTreeMap<OsString, OsString>,
    limit: Duration,
) -> KeychainProbe {
    use std::pin::pin;

    use futures::future::{Either, join3, select};
    use futures::io::AsyncWriteExt as _;

    let child_env: BTreeMap<OsString, OsString> = env
        .get_key_value(OsStr::new("HOME"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .into_iter()
        .collect();
    let Ok(spec) = clauseo_process::LaunchSpec::new(
        program.to_path_buf(),
        args,
        child_env,
        PathBuf::from("/"),
    ) else {
        return KeychainProbe::NotStarted;
    };
    let Ok(clauseo_process::Launched {
        mut stdin,
        stdout,
        stderr,
        mut handle,
        ..
    }) = launcher.launch(&spec)
    else {
        return KeychainProbe::NotStarted;
    };
    let ended = {
        let work = pin!(async {
            drop(stdin.close().await);
            let mut sink = futures::io::sink();
            let discard = futures::io::copy(stdout, &mut sink);
            let (_, _, report) = join3(discard, stderr.drain(), handle.exited()).await;
            report.code()
        });
        match select(work, pin!(timer.after(limit))).await {
            Either::Left((code, _)) => KeychainProbe::Exited(code),
            Either::Right(((), _)) => KeychainProbe::TimedOut,
        }
    };
    drop(handle);
    ended
}
