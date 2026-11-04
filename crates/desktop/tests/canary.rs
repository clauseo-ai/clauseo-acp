//! The usage counts carry no document text (S21, done-when 3): an end-to-end test with
//! `fake_agent` playing Claude Code plants a canary, a random token and the Devanagari matter
//! name "मेहरा तलाक़", everywhere a lawyer's words or a machine's names could come from: the
//! folder name, the papers' names and contents, the prompt, the agent's answer and a tool call
//! of it, the agent's stderr, an agent error and a refused command's error text, an environment
//! variable of the agent, the app's install path, `HOME`, `USER` and `CLAUSEO_DATA_DIR`. With
//! counts on (`FirstAct { counts: true }`) it runs a session through desktop's own counts code
//! (`counts_for_tests`), moves the counts' time past midnight and on into the week after, so
//! the day batch and the weekly tally are posted, and captures every body at a mock `Upload`.
//! No body may hold the canary in any form: raw bytes, gunzipped bytes, JSON-escaped (UTF-8 and
//! `\uXXXX`), percent-encoded, or inside a decoded JSON string. The bodies must also be real:
//! a `/v1/counts` batch with the session's `matter_opened`, `question_sent` and `answer_ended`
//! cells, so a counting path that silently broke fails the test too.
//!
//! S22 adds a forced crash to this same session (see `S22:` below), so the crash report's body
//! is searched for the canary with the same `support::leaks`.

// Test code throughout, so clippy allows its unwraps and panics (clippy.toml).
#![cfg(test)]

// A test crate root resolves modules beside itself, so the helper names its path.
#[path = "canary/support.rs"]
mod support;

use std::path::Path;
use std::sync::Arc;

use clauseo_counts::schema::{
    Agent, AnswerOutcome, Batch, Day, Delivery, MatterKind, QuestionSent, Time,
};
use clauseo_counts::{Counts, Mode, UploadPath};
use clauseo_desktop::counts_for_tests::{open, tick_once};
use clauseo_host::{Host, HostConfig, HostHandle};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{
    AgentId, CommandEnvelope, CommandId, CommandOutcome, DeliveryRequest, DomainEvent, HostCommand,
    HostError, HostEvent, MatterSource, Principal, PromptBlock, StopReason, StreamId, ThreadId,
    Title, TurnId,
};
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::StreamExt as _;

use support::{FakeDirs, FakeEnv, NoReader, RecordingUpload, Scratch, Seen, leaks, wait};

/// The matter's name, in Devanagari: a divorce matter, "Mehra divorce".
const MATTER: &str = "मेहरा तलाक़";

/// The Claude Code agent id: an agent the counts have a value for (`fake_agent` plays it).
const CLAUDE: &str = "claude-acp";

/// The agent environment variable that carries the canary.
const CANARY_VAR: &str = "CLAUSEO_CANARY_NOTE";

const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;

/// A random token from the OS random source: 32 hex digits no body could hold by chance.
fn token() -> String {
    format!(
        "cnry{:016x}{:016x}",
        clauseo_counts::os_seed(),
        clauseo_counts::os_seed()
    )
}

/// The `fake_agent` scenario, named `stem`: Claude Code's handshake; a first turn whose stderr,
/// failed tool call and answer hold `canary`; a second turn the agent fails with an error whose
/// message holds it.
fn scenario(stem: &str, canary: &str) -> String {
    use serde_json::json;
    let chunk = |id: &str, text: String| {
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk", "messageId": id,
            "content": {"type": "text", "text": text}}}})
    };
    let lines = [
        json!({"scenario": stem, "on_stdin_close": "exit",
            "description": "The canary test's agent: Claude Code's handshake, one turn whose stderr, failed tool call and answer hold the canary, then a turn failed with an error holding it"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1", "modes": {
            "currentModeId": "default", "availableModes": [
                {"id": "default", "name": "Manual"}, {"id": "acceptEdits", "name": "Accept edits"},
                {"id": "plan", "name": "Plan"}, {"id": "auto", "name": "Auto"}]}}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1", "params": {"sessionId": "sess-1"}}}),
        json!({"stderr": {"line": format!("reading the papers of {canary}")}}),
        json!({"update": {"update": {"sessionUpdate": "tool_call", "toolCallId": "tool-1",
            "title": format!("Read {canary}.txt"), "kind": "read", "status": "failed",
            "content": [{"type": "content", "content": {"type": "text",
                "text": format!("could not read {canary}")}}]}}}),
        chunk("m-answer-1", format!("The papers of {canary} say: ")),
        chunk("m-answer-1", format!("{canary}.")),
        json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p2", "params": {"sessionId": "sess-1"}}}),
        json!({"stderr": {"line": format!("failing on {canary}")}}),
        json!({"respond": {"to": "p2", "error": {"code": -32603,
            "message": format!("internal error in {canary}")}}}),
    ];
    let mut text = lines.map(|line| line.to_string()).join("\n");
    text.push('\n');
    text
}

/// `command` under a fresh id.
fn envelope(command: HostCommand) -> CommandEnvelope {
    CommandEnvelope {
        command_id: CommandId::new(uuid::Uuid::now_v7()).unwrap(),
        command,
    }
}

/// Runs `command`, which must succeed.
fn ok(host: &HostHandle, command: HostCommand) -> CommandOutcome {
    let what = format!("{command:?}");
    wait(&what, host.dispatch(envelope(command)))
        .unwrap_or_else(|err| panic!("{what} failed: {err}"))
}

/// Sends `text` to `thread` and waits for the turn to end; its stop.
fn turn(host: &HostHandle, thread: ThreadId, text: &str) -> StopReason {
    let mut events = wait(
        "subscribe_events",
        host.subscribe_events(Principal::LocalDesktop, StreamId::Thread(thread), None),
    )
    .unwrap();
    let outcome = ok(
        host,
        HostCommand::SendPrompt {
            thread,
            content: vec![PromptBlock::Text {
                text: text.to_owned(),
            }],
            delivery: DeliveryRequest::Queue,
        },
    );
    let CommandOutcome::PromptAccepted { turn, .. } = outcome else {
        panic!("SendPrompt answered {outcome:?}");
    };
    wait("TurnEnded", async {
        while let Some(event) = events.next().await {
            if let HostEvent::Events(envelopes) = event
                && let Some(stop) = envelopes.iter().find_map(|e| ended(&e.event, turn))
            {
                return stop;
            }
        }
        panic!("the thread's stream ended before its turn did");
    })
}

/// The stop of `turn` when `event` is its `TurnEnded`.
fn ended(event: &DomainEvent, turn: TurnId) -> Option<StopReason> {
    match event {
        DomainEvent::TurnEnded(ended) if ended.turn == turn => Some(ended.stop),
        _ => None,
    }
}

/// Imports a folder whose path does not exist (and holds the canary): an error that goes
/// through the host while counts are on.
fn refused_import(host: &HostHandle, missing: &Path, name: &str) -> HostError {
    let registered = wait("register_folder", host.register_folder(missing.to_owned()));
    let folder = match registered {
        Ok(folder) => folder,
        Err(err) => return err,
    };
    let created = wait(
        "CreateMatter of a missing folder",
        host.dispatch(envelope(HostCommand::CreateMatter {
            name: Title::new(name).unwrap(),
            source: MatterSource::ExistingFolder { folder },
            client: None,
            next_date: None,
        })),
    );
    match created {
        Err(err) => err,
        Ok(outcome) => panic!("a missing folder became a matter: {outcome:?}"),
    }
}

/// Waits until the open day holds what the session counted: a matter opened, two questions
/// and two answers ended (the place and the papers are counted off the actor).
fn until_counted(counts: &Counts) {
    wait("the session's counts", async {
        loop {
            let today = counts.today();
            let opened: u64 = today.matter_opened.iter().map(|c| u64::from(c.n)).sum();
            let asked: u64 = today.question_sent.iter().map(|c| u64::from(c.n)).sum();
            let answered: u64 = today.answer_ended.iter().map(|c| u64::from(c.n)).sum();
            if opened >= 1 && asked >= 2 && answered >= 2 {
                return;
            }
            async_io::Timer::after(std::time::Duration::from_millis(10)).await;
        }
    });
}

/// Ticks `counts` as the app's task does, every `step` seconds from `from` while before `to`.
fn tick_through(
    counts: &Counts,
    upload: &RecordingUpload,
    on: &(dyn Fn() -> bool + Sync),
    from: Time,
    to: Time,
    step: u64,
) {
    let mut now = from;
    while now.unix_secs() < to.unix_secs() {
        futures::executor::block_on(tick_once(counts, upload, now, on));
        now = now.plus(step);
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one session told in order: plant, run, close the day and the week, search"
)]
fn no_canary_reaches_a_usage_counts_body() {
    let token = token();
    let user = format!("{token}-मेहरा");
    let canary = format!("{MATTER} {token}");
    let scratch = Scratch::new(&format!(
        "clauseo-canary-{}-{token} {MATTER}",
        std::process::id()
    ));

    // HOME, USER, CLAUSEO_DATA_DIR and the install path, each holding the canary.
    let home = scratch.dir(&format!("home {canary}"));
    let data_dir = scratch.path().join(format!("data {canary}"));
    let locations =
        Locations::resolve(&FakeDirs { home: home.clone() }, &FakeEnv { data_dir }).unwrap();
    assert!(locations.data_dir().display().to_string().contains(&token));
    let data_dir = scratch.dir(&format!("data {canary}"));
    let exe = scratch
        .dir(&format!("Clauseo {canary}"))
        .join("clauseo-desktop");

    // The agent: fake_agent as Claude Code, playing a scenario that holds the canary, with the
    // canary in an environment variable, HOME and USER.
    let bin = clauseo_fake_agent::bin_path()
        .unwrap_or_else(|e| panic!("{e} (run cargo build -p clauseo-fake_agent)"));
    let stem = format!("canary-{token}");
    let scenario_path = scratch
        .dir(&format!("scenario {canary}"))
        .join(format!("{stem}.jsonl"));
    Scratch::write(&scenario_path, scenario(&stem, &canary).as_bytes());
    let settings = serde_json::json!({
        "agents": { CLAUDE: {
            "command": bin.display().to_string(),
            "args": ["--scenario", scenario_path.display().to_string()],
            "env": {
                CANARY_VAR: canary,
                "HOME": home.display().to_string(),
                "USER": user,
            },
        }},
        "matters": { "default_location": scratch.dir(&format!("matters {canary}")).display().to_string() },
    });
    Scratch::write(locations.settings_file(), settings.to_string().as_bytes());
    assert_eq!(locations.data_dir(), data_dir);

    // The matter's folder and papers: names and contents hold the canary.
    let folder = scratch.dir(&format!("work/{canary}"));
    Scratch::write(
        &folder.join(format!("{canary} petition.txt")),
        format!("Petition in {canary}\n").as_bytes(),
    );
    Scratch::write(
        &folder.join(format!("{token}.txt")),
        format!("Notes on {MATTER}: {token}\n").as_bytes(),
    );

    // The counts, opened as the app opens them, ticked at a fixed day's noon: the counts take
    // time as an input, so the test moves it while the host keeps its real clock.
    let effective = Effective::load(&locations, &SystemFiles, &[]);
    let counts = open(&locations, &effective, Some(&exe))
        .expect("the schema has a value for this OS and CPU");
    let upload = RecordingUpload::default();
    let on = || {
        Effective::load(&locations, &SystemFiles, &[])
            .settings()
            .privacy
            .counts
    };
    let noon = Day::from_ymd(2026, 11, 4).unwrap().start().plus(12 * HOUR);
    futures::executor::block_on(tick_once(&counts, &upload, noon, &on));

    let launcher = PassthroughLauncher::new(SpawnOptions::new().home(home.clone()));
    let config = HostConfig::new(effective, Arc::new(launcher))
        .with_counts(counts.clone())
        .with_limits_reader(Arc::new(NoReader))
        .with_home_folder(Some(home))
        .with_managed_mcp_path(scratch.path().join(format!("managed-mcp {canary}.json")));
    let (host, join) = Host::start(config).unwrap();

    ok(&host, HostCommand::FirstAct { counts: true });
    assert_eq!(counts.mode(), Mode::Saving);
    let folder_ref = wait("register_folder", host.register_folder(folder)).unwrap();
    let created = ok(
        &host,
        HostCommand::CreateMatter {
            name: Title::new(&canary).unwrap(),
            source: MatterSource::ExistingFolder { folder: folder_ref },
            client: None,
            next_date: None,
        },
    );
    let CommandOutcome::MatterCreated { matter } = created else {
        panic!("CreateMatter answered {created:?}");
    };
    let created = ok(
        &host,
        HostCommand::CreateThread {
            matter,
            agent: AgentId::new(CLAUDE).unwrap(),
        },
    );
    let CommandOutcome::ThreadCreated { thread } = created else {
        panic!("CreateThread answered {created:?}");
    };
    let first = turn(
        &host,
        thread,
        &format!("Summarise the petition in {canary} for {user}."),
    );
    assert_eq!(first, StopReason::EndTurn);
    let second = turn(&host, thread, &format!("Now list the dates in {canary}."));
    assert_eq!(second, StopReason::Failed);
    let refused = refused_import(
        &host,
        &scratch.path().join(format!("missing {canary}")),
        &canary,
    );
    // Its text names the missing folder, canary and all (checked so the error is the one meant).
    assert!(
        refused.to_string().contains(&token),
        "an error without the folder's path: {refused}"
    );
    until_counted(&counts);

    // S22: the forced crash goes here, while the session is open, and its report's body is
    // searched with `leaks` beside the counts' bodies below.

    wait("shutdown", host.shutdown()).unwrap();
    join.join().unwrap();

    // The day closes at midnight and its batch goes 0 to 10 minutes later; the week closes on
    // Monday the 9th and its tally goes at a moment in the week after it.
    let midnight = noon.plus(12 * HOUR);
    tick_through(
        &counts,
        &upload,
        &on,
        noon.plus(60),
        midnight.plus(HOUR),
        60,
    );
    assert!(
        upload
            .seen()
            .iter()
            .any(|seen| seen.path == UploadPath::Counts),
        "no /v1/counts POST within an hour of midnight: {:?}",
        upload.seen()
    );
    let next_week_end = Day::from_ymd(2026, 11, 17).unwrap().start().plus(DAY);
    tick_through(
        &counts,
        &upload,
        &on,
        midnight.plus(HOUR),
        next_week_end,
        600,
    );
    let seen = upload.seen();

    // The search: no form of the canary in any body.
    let upper = token.to_uppercase();
    let secrets = [
        token.as_str(),
        upper.as_str(),
        canary.as_str(),
        MATTER,
        "मेहरा",
        "तलाक़",
        user.as_str(),
        CANARY_VAR,
    ];
    let hits: Vec<String> = seen.iter().flat_map(|seen| leaks(seen, &secrets)).collect();
    assert!(
        hits.is_empty(),
        "the canary reached a body:\n{}",
        hits.join("\n")
    );

    // The bodies are real: the day batch holds what the session counted.
    let batches: Vec<Batch> = seen
        .iter()
        .filter(|seen| seen.path == UploadPath::Counts)
        .map(|seen: &Seen| serde_json::from_slice(&support::gunzip(&seen.body)).unwrap())
        .collect();
    let day = batches
        .iter()
        .flat_map(|batch| &batch.days)
        .find(|report| report.day == noon.day().unwrap())
        .unwrap_or_else(|| panic!("no report of the session's day in {batches:?}"));
    let cells = &day.cells;
    assert!(!cells.app_opened.is_empty(), "{cells:?}");
    assert!(
        cells
            .matter_opened
            .iter()
            .any(|cell| cell.key.kind == MatterKind::Folder),
        "{cells:?}"
    );
    let asked: u64 = cells
        .question_sent
        .iter()
        .filter(|cell| {
            cell.key
                == QuestionSent {
                    agent: Agent::Claude,
                    delivery: Delivery::Now,
                }
        })
        .map(|cell| u64::from(cell.n))
        .sum();
    assert_eq!(asked, 2, "{cells:?}");
    let outcomes: Vec<AnswerOutcome> = cells
        .answer_ended
        .iter()
        .filter(|cell| cell.key.agent == Agent::Claude)
        .map(|cell| cell.key.outcome)
        .collect();
    assert!(outcomes.contains(&AnswerOutcome::Done), "{cells:?}");
    assert!(outcomes.contains(&AnswerOutcome::Error), "{cells:?}");
    assert!(
        seen.iter().any(|seen| seen.path == UploadPath::Tally),
        "no /v1/tally POST by {next_week_end:?}: {seen:?}"
    );
}
