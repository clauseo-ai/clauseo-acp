//! A thread's kept model and effort (`actor/turn/picks/own.rs`), with the stand-in agent as
//! Codex: a new thread's prefs that its first list lacks are kept and told once
//! (`pickers-kept-created-unlisted`).

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentKind, AgentPrefs, FieldChange, HostCommand, MatterId, NoticeKind, StreamId,
};

use crate::codex::CODEX;
use crate::idle_stop::LIMIT;
use crate::matters::summary;
use crate::pickers::picks;
use crate::pickers_idle::started_and_notices;
use crate::pickers_kept::{created, names, run, stop_idle};
use crate::support::{Dir, Running, Scenario};

/// The matter's prefs for Codex: `pairs`.
fn codex_prefs(pairs: &[(&str, &str)]) -> AgentPrefs {
    AgentPrefs(BTreeMap::from([(AgentKind::Codex, picks(pairs))]))
}

/// Sets `matter`'s prefs to `prefs`.
fn set_prefs(host: &Running, matter: MatterId, prefs: AgentPrefs) {
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        skill_packs: FieldChange::Keep,
        pinned: None,
        agent_prefs: FieldChange::Set(prefs),
    });
}

/// The one notice of a session that runs `model` and `effort` instead of the kept values.
fn told(model: &str, effort: &str) -> NoticeKind {
    NoticeKind::PickNotUsed {
        model: Some(model.to_owned()),
        effort: Some(effort.to_owned()),
    }
}

/// A new thread whose matter's prefs hold the model gpt-6-luna alone, and whose first list
/// lacks it: the thread keeps gpt-6-luna (the first session's medium fills the effort), the
/// first answer says once that it runs GPT-6, the prefs keep it, and the relaunch whose list has
/// it again sets it back with no other notice.
#[test]
fn a_new_threads_prefs_its_first_list_lacks_are_kept_and_told_once() {
    let dir = Dir::new(
        "pickers_kept_created_unlisted",
        &[(CODEX, Scenario::Local("pickers-kept-created-unlisted"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let luna = [("model", "gpt-6-luna")];
    set_prefs(&host, matter, codex_prefs(&luna));
    let thread = created(&host, matter, None);
    let mut events = host.events(StreamId::Thread(thread));
    let first = run(&host, &mut events, thread, "Summarise clause 4.");
    run(&host, &mut events, thread, "And clause 5?");
    let (started, notices) = started_and_notices(&events, first);
    assert_eq!(started, names("GPT-6", "Medium"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "Medium")], "{}", events.dump());
    assert_eq!(summary(&host, matter).agent_prefs, Some(codex_prefs(&luna)));
    stop_idle(&host, &mut events);

    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let third = run(&host, &mut events, thread, "And clause 6?");
    let (started, notices) = started_and_notices(&events, third);
    assert_eq!(started, names("GPT-6 Luna", "Medium"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "Medium")], "{}", events.dump());
    host.shutdown();
}
