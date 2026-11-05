//! Skill packs turned on and off (`SetSkillPack`) and listed (`ListSkillPacks`), decision record
//! 0040's "Turning packs on (PR-E)": the codes (`settings.locked` when a firm decided,
//! `settings.refused` otherwise), the read off the host thread, a pack that changed, and what a
//! window may see of a pack's text.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use clauseo_protocol::{
    ErrorCode, HostCommand, HostEvent, HostQuery, PackId, PackReason, PackRefusal, PackState,
    PackStatus, QueryResult,
};

use crate::settings::subscribe_app;
use crate::support::{Dir, Running};
use crate::write;

const KIT: &str = "nda-kit";

fn kit() -> PackId {
    PackId::new(KIT).unwrap()
}

fn on(id: &PackId) -> HostCommand {
    HostCommand::SetSkillPack {
        id: id.clone(),
        on: true,
    }
}

fn off(id: &PackId) -> HostCommand {
    HostCommand::SetSkillPack {
        id: id.clone(),
        on: false,
    }
}

fn packs(dir: &Dir) -> PathBuf {
    dir.data().join("skill-packs")
}

/// Writes a valid pack `id` named `name`, with one skill `review`.
fn put_pack(dir: &Dir, id: &str, name: &str) {
    let root = packs(dir).join(id);
    let manifest = serde_json::json!({
        "format": 1,
        "id": id,
        "name": name,
        "version": "1.0.0",
        "author": "Example Chambers",
        "source_url": "https://example.org/pack",
        "licence": "CC-BY-4.0",
        "last_reviewed": "2026-10-20",
        "instruction_only": true,
        "skills": [{"name": "review"}],
    });
    write::write(
        &root.join("clauseo-pack.json"),
        manifest.to_string().as_bytes(),
    );
    write::write(
        &root.join("skills/review/SKILL.md"),
        b"---\nname: review\ndescription: Reviews it.\n---\n# Review\n",
    );
}

/// The hash the pack `id` has now, as the profile reads it.
fn hash_now(dir: &Dir, id: &str) -> String {
    clauseo_profile::packs::inspect(&packs(dir), id, false)
        .unwrap()
        .hash()
        .as_str()
        .to_owned()
}

/// A start whose `settings.json` holds `skills`, and `policy` when given.
fn start(dir: &Dir, skills: &serde_json::Value, policy: Option<&str>) -> Running {
    let settings = serde_json::json!({ "skills": skills });
    dir.set_settings(&[], settings);
    match policy {
        Some(policy) => dir.start_with_policy(0, policy),
        None => dir.start(0),
    }
}

/// A start with `extra: any` and a hook between each pack's two reads.
fn start_hooked(dir: &Dir, hook: Hook) -> Running {
    dir.set_settings(&[], serde_json::json!({ "skills": {"extra": "any"} }));
    dir.start_with(0, |config| config.with_pack_read_hook(hook))
}

/// `skills.enabled` as `settings.json` holds it.
fn enabled(dir: &Dir) -> serde_json::Value {
    let settings: serde_json::Value = serde_json::from_str(&dir.settings_json()).unwrap();
    settings["skills"]["enabled"].clone()
}

fn list(host: &Running) -> Vec<PackStatus> {
    match host.query(HostQuery::ListSkillPacks).unwrap() {
        QueryResult::SkillPacks(packs) => packs,
        other => panic!("ListSkillPacks answered {other:?}"),
    }
}

fn status<'a>(listed: &'a [PackStatus], id: &str) -> &'a PackStatus {
    listed
        .iter()
        .find(|status| status.id.as_str() == id)
        .unwrap_or_else(|| panic!("{id} is not listed: {listed:?}"))
}

/// Records the name of each thread the hook runs on.
/// A hook run between a pack's two reads.
type Hook = Arc<dyn Fn(&str) + Send + Sync>;

fn thread_names() -> (Arc<Mutex<Vec<String>>>, Hook) {
    let names = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&names);
    let hook = Arc::new(move |_id: &str| {
        let name = std::thread::current().name().unwrap_or("").to_owned();
        seen.lock().unwrap().push(name);
    });
    (names, hook)
}

fn assert_off_actor(names: &Mutex<Vec<String>>) {
    let names = names.lock().unwrap();
    assert!(!names.is_empty(), "no pack was read");
    for name in names.iter() {
        assert_eq!(name, "clauseo-codex-fs", "a pack was read on {name}");
    }
}

#[test]
fn turning_a_pack_on_records_its_hash_off_the_actor() {
    let dir = Dir::new("packs_on", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let (names, hook) = thread_names();
    let host = start_hooked(&dir, hook);
    let (mut sub, _first) = subscribe_app(&host);
    host.ok(on(&kit()));
    assert_off_actor(&names);
    assert_eq!(enabled(&dir)[KIT], hash_now(&dir, KIT));
    let events = crate::settings::ready(&mut sub);
    assert!(
        matches!(events.as_slice(), [HostEvent::SettingsChanged(_)]),
        "{events:?}"
    );
    assert_eq!(status(&list(&host), KIT).state, PackState::On);
}

#[test]
fn turning_on_again_after_a_change_records_the_new_hash() {
    let dir = Dir::new("packs_again", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let host = start(&dir, &serde_json::json!({"extra": "any"}), None);
    host.ok(on(&kit()));
    let first = hash_now(&dir, KIT);
    put_pack(&dir, KIT, "NDA kit, second edition");
    let second = hash_now(&dir, KIT);
    assert_ne!(first, second);
    assert_eq!(status(&list(&host), KIT).state, PackState::Changed);
    host.ok(on(&kit()));
    assert_eq!(enabled(&dir)[KIT], second);
    assert_eq!(status(&list(&host), KIT).state, PackState::On);
}

#[test]
fn turning_on_a_pack_policy_does_not_admit_is_refused() {
    let dir = Dir::new("packs_curated", &[]);
    put_pack(&dir, KIT, "NDA kit");
    put_pack(&dir, "lease-kit", "Lease kit");
    let policy = r#"{"skills": {"extra": "curated", "curated": ["lease-kit"]}}"#;
    let host = start(&dir, &serde_json::json!({}), Some(policy));
    host.refused(on(&kit()), ErrorCode::SettingsLocked);
    host.ok(on(&PackId::new("lease-kit").unwrap()));
    assert!(enabled(&dir).get(KIT).is_none());
    let listed = list(&host);
    assert_eq!(status(&listed, KIT).state, PackState::NotAdmitted);
    assert_eq!(
        status(&listed, KIT).reason,
        Some(PackReason::NotAdmitted { by_policy: true })
    );
    assert_eq!(status(&listed, "lease-kit").state, PackState::On);
}

#[test]
fn a_policy_off_refuses_with_settings_locked() {
    let dir = Dir::new("packs_policy_off", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let policy = r#"{"skills": {"extra": "off"}}"#;
    let host = start(&dir, &serde_json::json!({"extra": "any"}), Some(policy));
    host.refused(on(&kit()), ErrorCode::SettingsLocked);
    assert!(enabled(&dir).is_null());
}

#[test]
fn the_lawyers_own_extra_off_refuses_with_settings_refused() {
    let dir = Dir::new("packs_own_off", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let host = start(&dir, &serde_json::json!({"extra": "off"}), None);
    host.refused(on(&kit()), ErrorCode::SettingsRefused);
    let listed = list(&host);
    assert_eq!(
        status(&listed, KIT).reason,
        Some(PackReason::NotAdmitted { by_policy: false })
    );
}

#[test]
fn turning_on_an_invalid_pack_is_refused_with_its_code() {
    let dir = Dir::new("packs_invalid", &[]);
    put_pack(&dir, KIT, "NDA kit");
    write::write(
        &packs(&dir).join(KIT).join("skills/review/run.sh"),
        b"echo hi\n",
    );
    let host = start(&dir, &serde_json::json!({"extra": "any"}), None);
    let error = host.refused(on(&kit()), ErrorCode::SettingsRefused);
    assert!(!error.retryable(), "{error}");
    assert!(error.message().contains("(script"), "{error}");
    let missing = host.refused(
        on(&PackId::new("gone-kit").unwrap()),
        ErrorCode::SettingsRefused,
    );
    assert!(missing.message().contains("(missing)"), "{missing}");
    assert!(enabled(&dir).is_null());
    let listed = list(&host);
    assert_eq!(status(&listed, KIT).state, PackState::Invalid);
    assert!(matches!(
        status(&listed, KIT).reason,
        Some(PackReason::Invalid {
            kind: PackRefusal::Script | PackRefusal::ScriptInInstructionOnly
        })
    ));
}

#[test]
fn turning_off_removes_the_entry() {
    let dir = Dir::new("packs_off", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let host = start(&dir, &serde_json::json!({"extra": "any"}), None);
    host.ok(on(&kit()));
    assert!(enabled(&dir).get(KIT).is_some());
    host.ok(off(&kit()));
    assert!(enabled(&dir).get(KIT).is_none());
    assert_eq!(status(&list(&host), KIT).state, PackState::Off);
}

#[test]
fn a_locked_enabled_refuses_the_command() {
    let dir = Dir::new("packs_locked", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let policy = r#"{"skills": {"extra": "any", "enabled": {}}}"#;
    let host = start(&dir, &serde_json::json!({}), Some(policy));
    host.refused(on(&kit()), ErrorCode::SettingsLocked);
    host.refused(off(&kit()), ErrorCode::SettingsLocked);
}

#[test]
fn list_shows_changed_for_a_stale_hash() {
    let dir = Dir::new("packs_stale", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let stale = format!("sha256:{}", "0".repeat(64));
    let skills = serde_json::json!({"extra": "any", "enabled": {KIT: stale}});
    let host = start(&dir, &skills, None);
    let listed = list(&host);
    let kit = status(&listed, KIT);
    assert_eq!(kit.state, PackState::Changed);
    assert_eq!(kit.reason, None);
    assert_eq!(kit.hash.as_deref(), Some(hash_now(&dir, KIT).as_str()));
}

/// Off, not admitted, and a pack turned on whose folder is gone (`invalid`, `missing`).
#[test]
fn list_shows_off_and_a_missing_pack_as_invalid() {
    let dir = Dir::new("packs_states", &[]);
    put_pack(&dir, KIT, "NDA kit");
    write::create_dir_all(&packs(&dir).join("Not An Id"));
    let hash = format!("sha256:{}", "a".repeat(64));
    let skills = serde_json::json!({"extra": "any", "enabled": {"gone-kit": hash}});
    let host = start(&dir, &skills, None);
    let listed = list(&host);
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert_eq!(status(&listed, KIT).state, PackState::Off);
    assert_eq!(status(&listed, KIT).name.as_deref(), Some("NDA kit"));
    let gone = status(&listed, "gone-kit");
    assert_eq!(gone.state, PackState::Invalid);
    assert_eq!(
        gone.reason,
        Some(PackReason::Invalid {
            kind: PackRefusal::Missing
        })
    );
    assert_eq!(gone.name, None);
}

#[test]
fn list_reads_no_file_on_the_actor() {
    let dir = Dir::new("packs_list_thread", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let (names, hook) = thread_names();
    let host = start_hooked(&dir, hook);
    assert_eq!(list(&host).len(), 1);
    assert_off_actor(&names);
}

#[test]
fn a_pack_that_changes_while_hashed_is_refused_and_listed_as_changed() {
    let dir = Dir::new("packs_mid_hash", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let readme: PathBuf = packs(&dir).join(KIT).join("README.md");
    let count = Arc::new(Mutex::new(0_u32));
    let hook = Arc::new(move |_id: &str| {
        // Each read pair sees the pack change between its two reads.
        let mut count = count.lock().unwrap();
        *count += 1;
        write::write(&readme, format!("# edit {count}\n").as_bytes());
    });
    // The README the hook edits is part of the pack from the start.
    write::write(&packs(&dir).join(KIT).join("README.md"), b"# start\n");
    let host = start_hooked(&dir, hook);
    let error = host.refused(on(&kit()), ErrorCode::SettingsRefused);
    assert!(error.message().contains("(changed)"), "{error}");
    assert!(enabled(&dir).is_null());
    let listed = list(&host);
    assert_eq!(status(&listed, KIT).state, PackState::Changed);
    assert_eq!(status(&listed, KIT).hash, None);
}

#[test]
fn list_strings_are_filtered() {
    let dir = Dir::new("packs_filtered", &[]);
    // A right-to-left override and a zero-width space: format characters a window never shows.
    put_pack(&dir, KIT, "NDA \u{202E}tik\u{200B}");
    let host = start(&dir, &serde_json::json!({"extra": "any"}), None);
    let listed = list(&host);
    let kit = status(&listed, KIT);
    assert_eq!(kit.name, None);
    assert_eq!(kit.author.as_deref(), Some("Example Chambers"));
    assert_eq!(kit.source_url.as_deref(), Some("https://example.org/pack"));
    assert_eq!(kit.instruction_only, Some(true));
}

#[test]
fn the_command_error_names_only_the_id() {
    let dir = Dir::new("packs_error_text", &[]);
    put_pack(&dir, KIT, "Secret Pack Name");
    let pack: &Path = &packs(&dir).join(KIT);
    write::write(&pack.join("skills/review/payload.exe"), b"MZ secret bytes");
    let host = start(&dir, &serde_json::json!({"extra": "any"}), None);
    let error = host.refused(on(&kit()), ErrorCode::SettingsRefused);
    let message = error.message();
    assert!(message.contains(KIT), "{message}");
    for leak in [
        "Secret Pack Name",
        "payload",
        "secret bytes",
        "skill-packs",
        &dir.data().display().to_string(),
    ] {
        assert!(!message.contains(leak), "{message} holds {leak}");
    }
}

/// A hook that stops the first read: it says so on the first channel, then waits for the
/// second. Later reads pass.
fn blocking_hook() -> (Hook, Receiver<()>, Sender<()>) {
    let (entered, entered_rx) = channel();
    let (release, release_rx) = channel::<()>();
    let gate = Mutex::new(Some((entered, release_rx)));
    let hook: Hook = Arc::new(move |_id: &str| {
        let first = gate.lock().unwrap().take();
        if let Some((entered, release_rx)) = first {
            entered.send(()).unwrap();
            release_rx.recv().unwrap();
        }
    });
    (hook, entered_rx, release)
}

/// `on` from another thread, whose answer comes back on the handle.
fn on_in_background(host: &Running) -> std::thread::JoinHandle<clauseo_protocol::CommandOutcome> {
    let handle = host.handle.clone();
    std::thread::spawn(move || {
        let envelope = clauseo_protocol::CommandEnvelope {
            command_id: crate::support::command_id(),
            command: on(&kit()),
        };
        crate::support::wait("on", handle.dispatch(envelope)).unwrap()
    })
}

/// An `off` sent while an `on`'s read runs wins: the `on` answers `Done` and writes nothing.
#[test]
fn an_off_during_an_ons_read_wins() {
    let dir = Dir::new("packs_off_during_on", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let (hook, entered, release) = blocking_hook();
    let host = start_hooked(&dir, hook);
    let turning_on = on_in_background(&host);
    entered.recv().unwrap();
    host.ok(off(&kit()));
    release.send(()).unwrap();
    assert_eq!(
        turning_on.join().unwrap(),
        clauseo_protocol::CommandOutcome::Done
    );
    host.settle();
    assert!(enabled(&dir).get(KIT).is_none(), "{}", dir.settings_json());
    assert_eq!(status(&list(&host), KIT).state, PackState::Off);
}

/// Two `on`s racing: the first is overtaken, the second records the hash, once.
#[test]
fn two_racing_ons_record_one_hash() {
    let dir = Dir::new("packs_two_ons", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let (hook, entered, release) = blocking_hook();
    let host = start_hooked(&dir, hook);
    let (mut sub, _first) = subscribe_app(&host);
    let first = on_in_background(&host);
    entered.recv().unwrap();
    host.ok(on(&kit()));
    release.send(()).unwrap();
    assert_eq!(
        first.join().unwrap(),
        clauseo_protocol::CommandOutcome::Done
    );
    host.settle();
    assert_eq!(enabled(&dir)[KIT], hash_now(&dir, KIT));
    let changed = crate::settings::ready(&mut sub)
        .iter()
        .filter(|event| matches!(event, HostEvent::SettingsChanged(_)))
        .count();
    assert_eq!(changed, 1);
}

/// The reviewer's repro: a firm's `skills.curated` alone does not make the lawyer's own
/// `extra: off` the firm's decision.
#[test]
fn a_firm_curated_list_with_the_lawyers_own_extra_off_is_refused_not_locked() {
    let dir = Dir::new("packs_curated_own_off", &[]);
    put_pack(&dir, KIT, "NDA kit");
    let policy = r#"{"skills":{"curated":["nda-kit"]}}"#;
    let host = start(&dir, &serde_json::json!({"extra": "off"}), Some(policy));
    host.refused(on(&kit()), ErrorCode::SettingsRefused);
    assert_eq!(
        status(&list(&host), KIT).reason,
        Some(PackReason::NotAdmitted { by_policy: false })
    );
}
