//! What a kept rule reaches, checked by real path at every use (decision record 0063,
//! "Commands"): a kept command whose file was swapped for a link out of the matter reaches the
//! lawyer; a `multi_agent` helper's identical command is answered by the host, while its other
//! requests reach the lawyer with no "Always" on a card that cannot show the command.

use clauseo_protocol::{PermissionKind, StopReason, StreamId};

use crate::always::{ALWAYS, keep, offers, resolutions, resolved_always};
#[cfg(unix)]
use crate::always::{CLAUDE, ask, bash_options, claude_scenario};
use crate::permissions::answer_from;
use crate::support::{Dir, Scenario, matter_with_thread};

/// A kept `Bash(wc -l notes.txt)`, then `notes.txt` swapped for a link to another matter's
/// file: the identical request reaches the lawyer, on a card that offers no "Always".
#[cfg(unix)]
#[test]
fn a_kept_command_never_follows_a_link_out_of_the_matter() {
    let dir = Dir::new("always_reach_link", &[]);
    let wc = serde_json::json!({"command": "wc -l notes.txt"});
    let turn = ask("wc", "Bash", &wc, &bash_options(), "reject").to_vec();
    let scenario = claude_scenario(&dir, "claude-reach-link", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let other = dir.folder("other", &[("advice.txt", "privileged\n")]);
    crate::write::symlink(&other.join("advice.txt"), &folder.join("notes.txt"));
    keep(&dir, matter, &["Bash(wc -l notes.txt)"]);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Count the lines.");
    answer_from(&host, &mut events, thread, 0, &["reject"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        resolutions(&events),
        [(
            ["allow-once", "reject"].map(String::from).to_vec(),
            Some(("reject".to_owned(), PermissionKind::RejectOnce))
        )],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [None]);
    host.shutdown();
}

/// `codex-always-helper`: with `Run(wc -l notes.txt)` and `Edit(*)` kept, a helper's requests
/// (the parent's session, a toolCallId never announced): its identical command is answered
/// `allow_once` by the host and logged; another command reaches the lawyer with no "Always"; the
/// kept command in a folder outside the matter reaches the lawyer; a write to a document Word
/// holds open is refused by the host.
#[test]
fn a_helpers_identical_command_is_answered_and_nothing_more() {
    const CODEX: &str = crate::codex::CODEX;
    let dir = Dir::new(
        "always_reach_helper",
        &[(CODEX, Scenario::Local("codex-always-helper"))],
    );
    let host = dir.start(0);
    let owner = "owner file: open in Word";
    let files = [("Invoice.docx", "one"), ("~$nvoice.docx", owner)];
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &files, CODEX);
    keep(&dir, matter, &["Run(wc -l notes.txt)", "Edit(*)"]);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Have a helper count the notes.");
    answer_from(&host, &mut events, thread, 1, &["allow_once", "decline"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let (cards, chosen): (Vec<_>, Vec<_>) = resolutions(&events).into_iter().unzip();
    let once = Some(("allow_once".to_owned(), PermissionKind::AllowOnce));
    let decline = Some(("decline".to_owned(), PermissionKind::RejectOnce));
    assert_eq!(
        chosen,
        [once.clone(), once, decline.clone(), decline],
        "{}",
        events.dump()
    );
    for card in &cards {
        assert!(!card.contains(&ALWAYS.to_owned()), "{}", events.dump());
    }
    assert_eq!(offers(&events), [None, None, None, None]);
    // The host's answer names the command it allowed, though the helper's call was never
    // logged: its transcript row can show it.
    let named: Vec<Option<String>> = resolved_always(&events)
        .into_iter()
        .map(|always| always.and_then(|offer| offer.shown))
        .collect();
    assert_eq!(
        named,
        [Some("wc -l notes.txt".to_owned()), None, None, None],
        "{}",
        events.dump()
    );
    host.shutdown();
}
