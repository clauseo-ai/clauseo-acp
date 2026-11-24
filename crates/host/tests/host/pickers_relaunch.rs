//! The thread's own model across a relaunch (`actor/turn/picks.rs`, `prompt_picks/send.rs`): a
//! Codex session that reports its options only after its start's answer, while the next prompt
//! waits at step 4, still gets the kept model and effort set back before it
//! (`pickers-codex-late`), and a Claude session that lists the kept model in its other form,
//! short name or full id, gets the form it lists (`pickers-claude-alias-short`,
//! `pickers-claude-alias-full`).

use crate::codex::CODEX;
use crate::pickers::{CLAUDE, current};
use crate::pickers_idle::{Relaunched, header_options, relaunch, started_and_notices};

/// A `TurnStarted`'s model and effort names.
type Names = (Option<String>, Option<String>);

/// The second turn's model and effort names and the header's model and effort values, after
/// `relaunched`'s run, with no `PickNotUsed` notice; the effort option is `effort`.
fn kept(relaunched: &Relaunched, effort: &str) -> (Names, [Option<String>; 2]) {
    let Relaunched {
        host,
        thread,
        events,
        second,
        ..
    } = relaunched;
    let (started, notices) = started_and_notices(events, *second);
    assert!(notices.is_empty(), "{}", events.dump());
    let shown = header_options(host, *thread);
    (started, [current(&shown, "model"), current(&shown, effort)])
}

#[test]
fn a_codex_relaunch_whose_options_come_late_sets_the_kept_model_and_effort_back() {
    let config = [("model", "gpt-6-luna"), ("reasoning_effort", "low")];
    let relaunched = relaunch(
        (CODEX, &config, Some("300")),
        "pickers_codex_late",
        "pickers-codex-late",
    );
    let (started, shown) = kept(&relaunched, "reasoning_effort");
    assert_eq!(
        started,
        (Some("GPT-6 Luna".to_owned()), Some("Low".to_owned()))
    );
    assert_eq!(
        shown,
        [Some("gpt-6-luna".to_owned()), Some("low".to_owned())]
    );
    relaunched.host.shutdown();
}

#[test]
fn a_kept_full_model_id_is_set_as_the_short_name_the_relaunch_lists() {
    let config = [("model", "claude-sonnet-5-5")];
    let relaunched = relaunch(
        (CLAUDE, &config, None),
        "pickers_alias_short",
        "pickers-claude-alias-short",
    );
    let (started, shown) = kept(&relaunched, "effort");
    assert_eq!(started.0.as_deref(), Some("Sonnet 5.5"));
    assert_eq!(shown[0].as_deref(), Some("sonnet"));
    relaunched.host.shutdown();
}

#[test]
fn a_kept_short_model_name_is_set_as_the_full_id_the_relaunch_lists() {
    let config = [("model", "sonnet")];
    let relaunched = relaunch(
        (CLAUDE, &config, None),
        "pickers_alias_full",
        "pickers-claude-alias-full",
    );
    let (started, shown) = kept(&relaunched, "effort");
    assert_eq!(started.0.as_deref(), Some("Sonnet 5.5"));
    assert_eq!(shown[0].as_deref(), Some("claude-sonnet-5-5"));
    relaunched.host.shutdown();
}
