//! A cancelled turn ends what it left running: a command the agent started for the turn in a
//! process group of its own (as Codex starts each command, and keeps it past `turn/interrupt`),
//! while what ran before the prompt (an MCP server's stand-in) and the agent stay; the agent's
//! stop then ends the stand-in too, though it is outside the agent's group. What the next turn
//! starts right after the cancel survives the sweep's second pass.
//!
//! A process the host kills stays its living parent's unreaped zombie on Unix, so no exit event
//! comes for it: [`gone_within`] asks the OS whether it still runs, under a deadline, and
//! [`until_covered`] waits the same way until a cutoff taken now covers a command (the clock's
//! unit is 10 ms on Linux), as one running for seconds before a Stop always is.

use std::path::Path;
use std::time::Duration;

use clauseo_protocol::{CommandOutcome, DomainEvent, HostCommand, StopReason, StreamId};

use crate::support::{Dir, FAKE, Scenario, matter_with_thread, wait};

/// The durable batch: armed by the first chunk, which the scenario sends once the command runs.
const DURABLE: Duration = Duration::from_millis(250);
/// The sweep's wait between its two passes, on the host's clock.
const GRACE: Duration = Duration::from_secs(2);
/// How long an ended process may take to stop running, in real time.
const GONE_WITHIN: Duration = Duration::from_secs(5);

/// The pid `spawn_child` wrote to `name` in the matter folder, the agent's working folder.
fn pid_in(folder: &Path, name: &str) -> u32 {
    let text =
        std::fs::read_to_string(folder.join(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
    text.trim()
        .parse()
        .unwrap_or_else(|e| panic!("{name} holds {text:?}: {e}"))
}

/// Whether `pid` stops running within [`GONE_WITHIN`] of real time, asked every 10 ms.
fn gone_within(pid: u32) -> bool {
    wait("a process to stop running", async {
        for _ in 0..GONE_WITHIN.as_millis() / 10 {
            if !clauseo_process::running(pid) {
                return true;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
        !clauseo_process::running(pid)
    })
}

/// Returns once a cutoff taken now would cover `pid`, asked every 10 ms for at most
/// [`GONE_WITHIN`].
fn until_covered(pid: u32) {
    let covered = wait("a cutoff to cover the command", async {
        for _ in 0..GONE_WITHIN.as_millis() / 10 {
            if clauseo_process::started_before_now(pid) {
                return true;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
        clauseo_process::started_before_now(pid)
    });
    assert!(covered, "no cutoff covers {pid}");
}

/// Whether `pid` keeps running for `span` of real time, asked every 10 ms.
fn runs_for(pid: u32, span: Duration) -> bool {
    wait("a process to keep running", async {
        for _ in 0..span.as_millis() / 10 {
            if !clauseo_process::running(pid) {
                return false;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
        clauseo_process::running(pid)
    })
}

/// Kills the children a failed assert leaves running.
struct Reap(Vec<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        for &pid in &self.0 {
            if clauseo_process::running(pid) {
                clauseo_process::kill(pid);
            }
        }
    }
}

#[test]
fn cancel_ends_the_command_the_turn_started() {
    let dir = Dir::new(
        "cancel_ends_the_command_the_turn_started",
        &[(FAKE, Scenario::Local("cancel-own-group-command"))],
    );
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Run the shell command: sleep 120");
    // The chunk comes after the command started, and after the server's stand-in did.
    host.wait_armed(DURABLE);
    let server = pid_in(&folder, "mcp.pid");
    let command = pid_in(&folder, "cmd.pid");
    let reap = Reap(vec![server, command]);
    assert!(
        clauseo_process::running(command),
        "the command is not running"
    );
    until_covered(command);

    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(first);
    assert_eq!(ended.stop, StopReason::Cancelled);
    assert!(
        gone_within(command),
        "the command the cancelled turn started still runs"
    );
    assert!(
        clauseo_process::running(server),
        "the process from before the prompt was ended"
    );

    // The same agent answers the next prompt: no relaunch, so no detach between the turns.
    let (second, _) = host.prompt(thread, "Go on.");
    let ended = events.until_turn_ended(second);
    assert_eq!(ended.stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    let detached = events.count(|e| matches!(e, DomainEvent::AgentSessionDetached(_)));
    assert_eq!(detached, 0, "the agent was stopped: {}", events.dump());

    // The agent's stop ends the stand-in, though it left the agent's process group.
    host.shutdown();
    assert!(
        gone_within(server),
        "the agent's stop left a descendant outside its group"
    );
    drop(reap);
}

#[test]
fn cancel_spares_the_next_turns_command() {
    let dir = Dir::new(
        "cancel_spares_the_next_turns_command",
        &[(FAKE, Scenario::Local("cancel-then-next-command"))],
    );
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Run the shell command: sleep 120");
    host.wait_armed(DURABLE);
    let command = pid_in(&folder, "cmd.pid");
    let mut reap = Reap(vec![command]);
    until_covered(command);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(first);
    assert_eq!(ended.stop, StopReason::Cancelled);

    // The next turn starts at once, and its command with it, inside the sweep's 2 s grace.
    let (second, _) = host.prompt(thread, "Run the next command.");
    let ended = events.until_turn_ended(second);
    assert_eq!(ended.stop, StopReason::EndTurn);
    let next = pid_in(&folder, "next.pid");
    reap.0.push(next);
    // The grace ends: the sweep's second pass runs.
    host.wait_armed(GRACE);
    host.clock.advance(GRACE);
    assert!(
        gone_within(command),
        "the command the cancelled turn started still runs"
    );
    assert!(
        runs_for(next, Duration::from_secs(3)),
        "the sweep ended the next turn's command"
    );
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
    assert!(gone_within(next), "the agent's stop left the next command");
    drop(reap);
}
