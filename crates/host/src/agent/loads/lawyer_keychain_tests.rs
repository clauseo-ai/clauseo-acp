//! A launch's Settings › Tools keychain reads on the `clauseo-codex-fs` thread: a read that
//! does not answer before the launch's wait runs out (a macOS permission prompt nobody answered)
//! fails as `agent.tool_secret_unreadable`; a read that is refused leaves its server out and
//! the launch goes on; a hang elsewhere in the same prep stays `agent.launch_failed`.

use std::sync::{Arc, Mutex, mpsc};

use clauseo_connectors::{SecretStore, StoreError};
use clauseo_protocol::{ErrorCode, MatterId, Timestamp};
use clauseo_settings::{Effective, Locations, SystemFiles};
use serde_json::json;

use super::McpControl;
use super::lawyer::{self, HostEnv, Lawyer, McpSecrets, Wanted};
use crate::ManualClock;
use crate::agent::codex::prepare::{Job, LAUNCH_WAIT, ToolSecretRead, off_actor_or_undo};
use crate::error::Error;
use crate::pipeline::tests::support::id;

/// A keychain whose every read waits until `release` is dropped, as macOS does while its
/// permission prompt is up; `entered` is told when a read starts.
struct Prompting {
    entered: async_channel::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl SecretStore for Prompting {
    fn get(&self, _account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let _told = self.entered.send_blocking(());
        let _released = self.release.lock().map(|release| release.recv());
        Ok(Some(b"late".to_vec()))
    }

    fn set(&self, _account: &str, _secret: &[u8]) -> Result<(), StoreError> {
        Ok(())
    }

    fn delete(&self, _account: &str) -> Result<(), StoreError> {
        Ok(())
    }
}

/// A keychain that refuses every read, as when the lawyer chooses Deny at the prompt.
struct Denying;

impl SecretStore for Denying {
    fn get(&self, _account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Err(StoreError::NoAccess("user canceled".to_owned()))
    }

    fn set(&self, _account: &str, _secret: &[u8]) -> Result<(), StoreError> {
        Ok(())
    }

    fn delete(&self, _account: &str) -> Result<(), StoreError> {
        Ok(())
    }
}

fn clock() -> ManualClock {
    ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap())
}

/// One local server, `github`, whose `TOKEN` is a keychain value.
fn wanted() -> Wanted {
    Wanted {
        accounts: vec!["github/env/TOKEN".to_owned()],
        commands: vec!["/bin/npx".to_owned()],
    }
}

/// A resolver that finds every command at `/bin/npx`.
fn found() -> lawyer::Resolve {
    Arc::new(|_typed: &str| Some("/bin/npx".into()))
}

/// Runs `work` as a launch's preparation, watched by `reading`, and lets the launch's wait run
/// out once `started` says the thread is in.
fn launch_timing_out<T: Send + 'static>(
    reading: ToolSecretRead,
    started: async_channel::Receiver<()>,
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    let clock = clock();
    let job = Job::launch(id::<MatterId>(7)).watching(reading);
    let prep = off_actor_or_undo(Arc::new(clock.clone()), job, work, drop);
    let move_on = async move {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, LAUNCH_WAIT);
        clock.advance(LAUNCH_WAIT);
    };
    let (result, ()) = futures::executor::block_on(futures::future::join(prep, move_on));
    result
}

#[test]
fn a_keychain_read_that_does_not_answer_in_time_is_the_tool_secrets_code() {
    let (entered, started) = async_channel::unbounded();
    let (release, blocked) = mpsc::channel::<()>();
    let store = Prompting {
        entered,
        release: Mutex::new(blocked),
    };
    let reading = ToolSecretRead::default();
    let marked = reading.clone();
    let result = launch_timing_out(reading, started, move || {
        let resolve = found();
        Ok(lawyer::read(
            Some(&store as &dyn SecretStore),
            &resolve,
            wanted(),
            &marked,
        ))
    });
    let error = result.unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentToolSecretUnreadable);
    assert!(error.code().is_retryable());
    drop(release);
}

/// The same prep hung before any keychain read (here a command's resolve on a share that does
/// not answer) is the folder's slowness: `agent.launch_failed`.
#[test]
fn a_hang_outside_the_keychain_read_stays_a_failed_launch() {
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let blocked = Mutex::new(blocked);
    let reading = ToolSecretRead::default();
    let marked = reading.clone();
    let result = launch_timing_out(reading, started, move || {
        let resolve: lawyer::Resolve = Arc::new(move |_typed: &str| {
            let _told = entered.send_blocking(());
            let _released = blocked.lock().map(|blocked| blocked.recv());
            None
        });
        Ok(lawyer::read(
            Some(&Denying as &dyn SecretStore),
            &resolve,
            wanted(),
            &marked,
        ))
    });
    assert_eq!(result.unwrap_err().code(), ErrorCode::AgentLaunchFailed);
    drop(release);
}

/// A refused read fails nothing: the prep answers, and the server that needed the value is left
/// out and named, which Settings › Tools shows as missing (`actor::mcp::left_out`).
#[test]
fn a_refused_keychain_read_leaves_its_server_out_and_the_launch_goes_on() {
    let reading = ToolSecretRead::default();
    let marked = reading.clone();
    let job = Job::launch(id::<MatterId>(7)).watching(reading);
    let prep = off_actor_or_undo(
        Arc::new(clock()),
        job,
        move || {
            let resolve = found();
            Ok(lawyer::read(
                Some(&Denying as &dyn SecretStore),
                &resolve,
                wanted(),
                &marked,
            ))
        },
        drop,
    );
    let secrets: McpSecrets = futures::executor::block_on(prep).unwrap();
    let root = std::env::temp_dir().join(format!("clauseo-denied-{}", std::process::id()));
    crate::write::create_dir_all(&root).unwrap();
    let servers = json!({"mcp": {"servers": {"github": {"command": "/bin/npx", "enabled": true,
        "env": {"TOKEN": {"keychain": true}}}}}});
    let text = servers.to_string();
    crate::write::write_file(&root.join("settings.json"), text.as_bytes()).unwrap();
    let effective = Effective::load(&Locations::under_root(&root), &SystemFiles, &[]);
    crate::write::remove_dir_all(&root).unwrap();
    let lawyer = Lawyer {
        control: McpControl::Clauseo,
        safe_workspace: false,
    };
    let host = HostEnv {
        path: None,
        windows_env: &[],
    };
    let (sent, left_out) = lawyer::servers(&effective, lawyer, &secrets, host);
    assert!(sent.is_empty(), "{sent:?}");
    assert_eq!(left_out, [("github".to_owned(), "TOKEN".to_owned())]);
}
