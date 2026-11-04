//! The host's usage-count call sites (decision record 0036): each event is counted once, from
//! the place `actor/counting.rs` names, into the counts `HostConfig::with_counts` gave. The
//! counts here are opened on and before the first act, so they live in memory and nothing is
//! written under `<data_dir>/counts/`; `Counts::today` reads them back.

use std::time::Duration;

use clauseo_counts::schema::{
    Agent, AnswerEnded, AnswerOutcome, AppOpened, Arch, Channel, DayCells, Delivery, Feature,
    FeatureUsed, InstallType, Language, Launch, MatterKind, MatterOpened, Papers, Place, Platform,
    Protection, QuestionSent, SettingChanged, TextSize, Theme, Version,
};
use clauseo_counts::{About, Counts, Mode};
use clauseo_protocol::{
    CheckpointKind, CommandOutcome, DomainEvent, FileChoice, FileDecision, HostCommand, MatterPath,
    MatterSource, PrivacyPatch, RestoreRequest, StopReason, StreamId, Title, UiPatch,
};

use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};
use crate::versions::matter_events;

/// The Claude Code agent id: an agent the counts have a value for (`fake_agent` plays it).
pub(crate) const CLAUDE: &str = "claude-acp";

/// The envelope values of every test's counts.
pub(crate) fn about() -> About {
    About {
        version: "3.0.4".parse::<Version>().unwrap(),
        channel: Channel::Stable,
        platform: Platform::macos(15).unwrap(),
        arch: Arch::Aarch64,
        language: Language::En,
        install_type: InstallType::PerUser,
        managed: false,
    }
}

/// The counts of `dir`'s data folder, as the desktop opens them: `on` is `privacy.counts` after
/// policy, and the first act has happened when its marker exists.
pub(crate) fn open_counts(dir: &Dir, on: bool) -> Counts {
    let consented = dir.data().join("first_act_done").exists();
    Counts::open(dir.data(), about(), on, consented, 7)
}

/// Starts run `run` of `dir` with `counts`.
pub(crate) fn start_counting(dir: &Dir, run: u16, counts: &Counts) -> Running {
    let counts = counts.clone();
    dir.start_with(run, move |config| config.with_counts(counts))
}

/// Each cell of `cells` with its count.
fn cells<K: Copy>(cells: &[clauseo_counts::schema::Cell<K>]) -> Vec<(K, u64)> {
    cells
        .iter()
        .map(|cell| (cell.key, u64::from(cell.n)))
        .collect()
}

/// Waits, in real time up to the watchdog, until `done`: for what a thread off the actor counts.
fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    wait(what, async {
        while !done() {
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
    });
}

/// The open day's cells, after the host has settled.
fn today(host: &Running, counts: &Counts) -> DayCells {
    host.settle();
    counts.today()
}

#[test]
fn app_opened_is_counted_once_per_start() {
    let dir = Dir::new("counts_app_opened", &[]);
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    assert_eq!(counts.mode(), Mode::Memory);
    let cold = AppOpened {
        launch: Launch::Cold,
    };
    assert_eq!(cells(&today(&host, &counts).app_opened), [(cold, 1)]);
    host.shutdown();
    let host = start_counting(&dir, 1, &counts);
    assert_eq!(cells(&today(&host, &counts).app_opened), [(cold, 2)]);
    host.shutdown();
    // Nothing was saved before the first act.
    assert!(!dir.data().join("counts").exists());
}

#[test]
fn no_counts_count_nothing_and_fail_nothing() {
    let dir = Dir::new(
        "counts_none",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the brief.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.shutdown();
}

#[test]
fn counts_off_count_nothing() {
    let dir = Dir::new(
        "counts_off",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    let counts = open_counts(&dir, false);
    let host = start_counting(&dir, 0, &counts);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the brief.");
    events.until_turn_ended(turn);
    assert_eq!(today(&host, &counts), DayCells::default());
    host.shutdown();
}

#[test]
fn matter_opened_counts_an_imported_folder_a_new_one_and_an_open() {
    let dir = Dir::new("counts_matter_opened", &[]);
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    let folder = dir.folder("imported", &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    let matter = host.create_matter("Imported", &folder);

    let parent = dir.folder("parent", &[]);
    let new_folder = host.folder_ref(&parent);
    let created = host.ok(HostCommand::CreateMatter {
        name: Title::new("New matter").unwrap(),
        source: MatterSource::NewFolder { folder: new_folder },
        client: None,
        next_date: None,
    });
    assert!(matches!(created, CommandOutcome::MatterCreated { .. }));
    host.ok(HostCommand::OpenMatter { matter });

    let opened = |kind, papers| MatterOpened {
        kind,
        papers,
        // Each test's folders are on a local disk.
        place: Some(Place::Local),
        protection: Protection::NotProtected,
    };
    let want = |cells: &DayCells| {
        let mut got = self::cells(&cells.matter_opened);
        got.sort();
        got
    };
    let mut expected = vec![
        // The import and the open, each with its two papers.
        (opened(MatterKind::Folder, Papers::UpTo10), 2),
        (opened(MatterKind::NewFolder, Papers::None), 1),
    ];
    expected.sort();
    // Each counts from a `clauseo-place` thread once the place is known; the open after its
    // listing on the matter's worker.
    let mut got = Vec::new();
    host.settle();
    eventually("three matter_opened", || {
        // No settle in here: `wait` is already running an executor.
        got = want(&counts.today());
        got.iter().map(|(_, n)| n).sum::<u64>() == 3
    });
    assert_eq!(got, expected);
    host.shutdown();
}

#[test]
fn question_sent_counts_the_agent_and_how_it_went() {
    let dir = Dir::new(
        "counts_question_sent",
        &[
            (CLAUDE, Scenario::Bundled("claude-prompt-stream")),
            (FAKE, Scenario::Bundled("prompt-stream")),
        ],
    );
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the brief.");
    events.until_turn_ended(turn);
    // An agent the schema has no value for is not counted.
    let other = host.create_thread(matter, FAKE);
    let mut other_events = host.events(StreamId::Thread(other));
    let (other_turn, _) = host.prompt(other, "Read the brief.");
    other_events.until_turn_ended(other_turn);
    let now = QuestionSent {
        agent: Agent::Claude,
        delivery: Delivery::Now,
    };
    assert_eq!(cells(&today(&host, &counts).question_sent), [(now, 1)]);
    host.shutdown();
}

/// One turn of `scenario` played as Claude Code, ended as `end` does; the turn's stop and the
/// `answer_ended` cells it counted.
fn answer_ended(
    test: &str,
    scenario: &'static str,
    end: impl FnOnce(&Running, clauseo_protocol::ThreadId),
) -> (StopReason, Vec<(AnswerEnded, u64)>) {
    let dir = Dir::new(test, &[(CLAUDE, Scenario::Bundled(scenario))]);
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the brief.");
    end(&host, thread);
    let stop = events.until_turn_ended(turn).stop;
    let ended = cells(&today(&host, &counts).answer_ended);
    host.shutdown();
    (stop, ended)
}

// One test over every stop (code owner's condition 5): `end::finish` counts each turn's end
// once, with its outcome, whichever way it ended.
#[test]
fn answer_ended_counts_each_stop_once_from_the_one_turn_end() {
    let nothing = |_: &Running, _| {};
    let cases: [(&str, &'static str, StopReason, AnswerOutcome); 4] = [
        (
            "counts_end_turn",
            "claude-prompt-stream",
            StopReason::EndTurn,
            AnswerOutcome::Done,
        ),
        (
            "counts_failed",
            "claude-exit-mid-turn",
            StopReason::Failed,
            AnswerOutcome::Error,
        ),
        (
            "counts_at_limit",
            "claude-limit-rejected",
            StopReason::AtLimit,
            AnswerOutcome::Limit,
        ),
        (
            "counts_signin_expired",
            "auth-required",
            StopReason::Failed,
            AnswerOutcome::SigninExpired,
        ),
    ];
    for (test, scenario, stop, outcome) in cases {
        let (stopped, ended) = answer_ended(test, scenario, nothing);
        assert_eq!(stopped, stop, "{test}");
        assert_eq!(ended.len(), 1, "{test}: {ended:?}");
        let (cell, n) = ended[0];
        assert_eq!(
            (cell.agent, cell.outcome, n),
            (Agent::Claude, outcome, 1),
            "{test}"
        );
    }

    // Cancelled: the lawyer stops it; the agent never answers the cancel, so it ends at 5 s.
    let (stopped, ended) = answer_ended(
        "counts_cancelled",
        "claude-cancel-unanswered",
        |host, thread| {
            host.wait_armed(Duration::from_millis(250));
            host.ok(HostCommand::Cancel { thread });
            host.wait_armed(Duration::from_secs(5));
            host.clock.advance(Duration::from_secs(5));
        },
    );
    assert_eq!(stopped, StopReason::Cancelled);
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert_eq!(ended[0].0.outcome, AnswerOutcome::StoppedByLawyer);
    assert_eq!(ended[0].1, 1);
}

#[test]
fn answer_ended_times_the_answer_and_its_first_words() {
    let (stop, ended) = answer_ended("counts_first_words", "claude-prompt-stream", |_, _| {});
    assert_eq!(stop, StopReason::EndTurn);
    let [(cell, 1)] = ended[..] else {
        panic!("{ended:?}");
    };
    // The host's clock does not move in this test: both are under a second.
    assert_eq!(cell.took, clauseo_counts::schema::Took::Lt1s);
    assert_eq!(cell.first_words, Some(clauseo_counts::schema::Took::Lt1s));
}

#[test]
fn feature_used_counts_an_undo_and_a_restore_once_each() {
    let dir = Dir::new(
        "counts_feature_used",
        &[(FAKE, Scenario::Local("undo-note"))],
    );
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    let (matter, thread, _) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Write a note.");
    events.until_turn_ended(first);
    let undo = HostCommand::UndoStep {
        thread,
        turn: first,
        files: vec![FileDecision {
            path: MatterPath::new("notes/a.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    };
    let id = crate::support::command_id();
    assert!(matches!(
        host.dispatch_as(id, undo.clone()).unwrap(),
        CommandOutcome::StepUndone { .. }
    ));
    events.until("StepUndone", |e| {
        e.position(|event| matches!(event, DomainEvent::StepUndone(_)))
            .is_some()
    });
    // The same command again answers from its receipt: not a second use.
    host.dispatch_as(id, undo).unwrap();

    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    let restore = HostCommand::RestoreVersion {
        matter,
        checkpoint: import,
        scope: RestoreRequest::Files {
            paths: vec![MatterPath::new("brief.txt").unwrap()],
        },
    };
    assert!(matches!(
        host.ok(restore),
        CommandOutcome::VersionRestored { .. }
    ));
    let used = |feature| FeatureUsed { feature };
    let mut got = cells(&today(&host, &counts).feature_used);
    got.sort();
    let mut expected = vec![(used(Feature::Undo), 1), (used(Feature::Restore), 1)];
    expected.sort();
    assert_eq!(got, expected);
    host.shutdown();
}

#[test]
fn setting_changed_counts_each_display_value_and_crash_reports_but_not_counts() {
    let dir = Dir::new("counts_setting_changed", &[]);
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    host.ok(HostCommand::SetUiSettings {
        patch: UiPatch {
            theme: Some(clauseo_protocol::Theme::Dark),
            font_size: Some(18),
            ..UiPatch::default()
        },
    });
    // The same value again changes nothing, so it is not counted.
    host.ok(HostCommand::SetUiSettings {
        patch: UiPatch {
            theme: Some(clauseo_protocol::Theme::Dark),
            ..UiPatch::default()
        },
    });
    host.ok(HostCommand::SetPrivacySettings {
        patch: PrivacyPatch {
            crash_reports: Some(true),
            counts: None,
        },
    });
    let mut got = cells(&today(&host, &counts).setting_changed);
    got.sort();
    let mut expected = vec![
        (SettingChanged::Theme(Theme::Dark), 1),
        (
            SettingChanged::TextSize(TextSize::try_from(18_u64).unwrap()),
            1,
        ),
        (SettingChanged::CrashReports(true), 1),
    ];
    expected.sort();
    assert_eq!(got, expected);
    // Counts' own switch is never counted: off forgets everything, and on starts afresh.
    host.ok(HostCommand::SetPrivacySettings {
        patch: PrivacyPatch {
            counts: Some(false),
            crash_reports: None,
        },
    });
    host.ok(HostCommand::SetPrivacySettings {
        patch: PrivacyPatch {
            counts: Some(true),
            crash_reports: None,
        },
    });
    assert_eq!(today(&host, &counts).setting_changed, []);
    host.shutdown();
}
