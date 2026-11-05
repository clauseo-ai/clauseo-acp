//! The launch's pack request, built on the actor from the matter's list and the `skills`
//! settings, and the log line's text.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clauseo_profile::packs::{PackEntry, PackHash, Refusal, Skip};
use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths};
use clauseo_protocol::{
    AgentKind, Notice, NoticeKind, NoticeLevel, PackId, PackList, PackSkipReason,
};
use clauseo_settings::{Locations, SkillsExtra, SkillsSettings};

use super::{PackNotices, PackPrep, PackRequest, PackSkip, skipped_text};
use crate::write;

/// A recorded hash: `sha256:` and 64 times `digit`.
fn hash(digit: char) -> String {
    format!("sha256:{}", digit.to_string().repeat(64))
}

/// `skills` with `extra`, `curated` and `enabled` (each id with its own hash).
fn skills(extra: SkillsExtra, curated: &[&str], enabled: &[(&str, char)]) -> SkillsSettings {
    SkillsSettings {
        extra,
        curated: curated.iter().map(|id| (*id).to_owned()).collect(),
        allow_scripts: false,
        enabled: enabled
            .iter()
            .map(|(id, digit)| ((*id).to_owned(), hash(*digit)))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn list(ids: &[&str]) -> PackList {
    PackList::new(ids.iter().map(|id| PackId::new(id).unwrap()).collect()).unwrap()
}

/// The ids the request loads.
fn ids(request: &PackRequest) -> Vec<&str> {
    request
        .entries()
        .iter()
        .map(|entry| entry.id.as_str())
        .collect()
}

const THREE: [(&str, char); 3] = [("nda-kit", '1'), ("lease-kit", '2'), ("tax-kit", '3')];

#[test]
fn a_missing_matter_list_uses_skills_enabled() {
    let request = PackRequest::new(&skills(SkillsExtra::Any, &[], &THREE), None);
    // `skills.enabled` order, which is the map's (sorted by id).
    assert_eq!(ids(&request), ["lease-kit", "nda-kit", "tax-kit"]);
    assert_eq!(
        request.entries()[1],
        PackEntry {
            id: "nda-kit".to_owned(),
            hash: PackHash::parse(&hash('1')).unwrap(),
        }
    );
    assert!(request.skipped.is_empty(), "{:?}", request.skipped);
}

#[test]
fn a_matter_list_narrows_the_global_list_and_empty_means_none() {
    let settings = skills(SkillsExtra::Any, &[], &THREE);
    let narrowed = PackRequest::new(&settings, Some(&list(&["tax-kit", "nda-kit"])));
    assert_eq!(ids(&narrowed), ["nda-kit", "tax-kit"]);
    assert!(narrowed.skipped.is_empty(), "{:?}", narrowed.skipped);
    let none = PackRequest::new(&settings, Some(&list(&[])));
    assert_eq!(none.entries(), []);
    assert!(none.skipped.is_empty(), "{:?}", none.skipped);
}

#[test]
fn an_id_not_turned_on_is_skipped() {
    let settings = skills(SkillsExtra::Any, &[], &THREE[..1]);
    let request = PackRequest::new(&settings, Some(&list(&["nda-kit", "wills-kit"])));
    assert_eq!(ids(&request), ["nda-kit"]);
    assert_eq!(
        request.skipped,
        [("wills-kit".to_owned(), PackSkip::NotTurnedOn)]
    );
}

#[test]
fn policy_off_loads_no_pack_and_curated_admits_only_listed_ids() {
    let off = PackRequest::new(&skills(SkillsExtra::Off, &["nda-kit"], &THREE), None);
    assert_eq!(off.entries(), []);
    assert_eq!(off.skipped.len(), 3);
    assert!(
        off.skipped
            .iter()
            .all(|(_, skip)| *skip == PackSkip::NotAdmitted)
    );
    let curated = skills(SkillsExtra::Curated, &["nda-kit", "wills-kit"], &THREE);
    let request = PackRequest::new(&curated, None);
    assert_eq!(ids(&request), ["nda-kit"]);
    assert_eq!(
        request.skipped,
        [
            ("lease-kit".to_owned(), PackSkip::NotAdmitted),
            ("tax-kit".to_owned(), PackSkip::NotAdmitted),
        ]
    );
    // A curated id that is not turned on stays off: curated admits, it never turns on.
    let narrowed = PackRequest::new(&curated, Some(&list(&["wills-kit"])));
    assert_eq!(narrowed.entries(), []);
    assert_eq!(
        narrowed.skipped,
        [("wills-kit".to_owned(), PackSkip::NotTurnedOn)]
    );
}

#[test]
fn allow_scripts_rides_on_the_request() {
    let mut settings = skills(SkillsExtra::Any, &[], &THREE);
    assert!(!PackRequest::new(&settings, None).allow_scripts());
    settings.allow_scripts = true;
    assert!(PackRequest::new(&settings, None).allow_scripts());
}

#[test]
fn the_log_text_holds_ids_and_codes_only() {
    let skipped = [
        ("nda-kit".to_owned(), PackSkip::Load(Skip::Changed)),
        (
            "lease-kit".to_owned(),
            PackSkip::Load(Skip::Refused(Refusal::Script)),
        ),
        ("wills-kit".to_owned(), PackSkip::NotTurnedOn),
        ("/home/x/Secret Pack".to_owned(), PackSkip::NotAdmitted),
    ];
    assert_eq!(
        skipped_text(&skipped),
        "nda-kit:changed, lease-kit:script, wills-kit:not_turned_on, <bad id>:not_admitted"
    );
}

// The notices, one per skipped pack: the lead, a space, the reason. The id sits in straight
// quotes; it is `PackId`-shaped (`pack_notices` drops any other), so it holds no quote.

#[test]
fn the_notice_for_a_pack_not_turned_on() {
    assert_eq!(
        PackSkip::NotTurnedOn.notice("nda-kit"),
        "Clauseo didn't load the skill pack \"nda-kit\". This matter is set to use it, but it isn't turned on."
    );
}

#[test]
fn the_notice_for_a_pack_not_admitted() {
    assert_eq!(
        PackSkip::NotAdmitted.notice("nda-kit"),
        "Clauseo didn't load the skill pack \"nda-kit\". The skill pack settings on this computer don't allow it."
    );
}

#[test]
fn the_notice_for_a_changed_pack() {
    assert_eq!(
        PackSkip::Load(Skip::Changed).notice("nda-kit"),
        "Clauseo didn't load the skill pack \"nda-kit\". Its files changed after it was turned on. Turn it on again to use it."
    );
}

#[test]
fn the_notice_for_a_missing_pack() {
    assert_eq!(
        PackSkip::Load(Skip::Refused(Refusal::Missing)).notice("nda-kit"),
        "Clauseo didn't load the skill pack \"nda-kit\". Clauseo couldn't find it on this computer."
    );
}

#[test]
fn the_notice_for_an_invalid_pack() {
    assert_eq!(
        PackSkip::Load(Skip::Refused(Refusal::Script)).notice("nda-kit"),
        "Clauseo didn't load the skill pack \"nda-kit\". Its files aren't a valid skill pack."
    );
}

#[test]
fn the_notice_for_a_pack_past_the_cap() {
    assert_eq!(
        PackSkip::Load(Skip::TooMany).notice("nda-kit"),
        "Clauseo didn't load the skill pack \"nda-kit\". Clauseo loads at most 16 skill packs at a time."
    );
}

fn nda_kit() -> PackId {
    PackId::new("nda-kit").unwrap()
}

#[test]
fn each_skip_reads_as_one_of_six_reasons() {
    let cases = [
        (PackSkip::NotTurnedOn, PackSkipReason::NotTurnedOn),
        (PackSkip::NotAdmitted, PackSkipReason::NotAdmitted),
        (PackSkip::Load(Skip::Changed), PackSkipReason::Changed),
        (PackSkip::Load(Skip::TooMany), PackSkipReason::TooMany),
        (
            PackSkip::Load(Skip::Refused(Refusal::Missing)),
            PackSkipReason::Missing,
        ),
        (
            PackSkip::Load(Skip::Refused(Refusal::Script)),
            PackSkipReason::Invalid,
        ),
    ];
    for (skip, reason) in cases {
        assert_eq!(skip.reason(), reason);
    }
}

#[test]
fn a_host_notice_has_its_kind_its_pack_and_its_reason() {
    let notice = PackSkip::NotTurnedOn.host_notice(nda_kit());
    assert_eq!(
        notice,
        Notice {
            level: NoticeLevel::Warning,
            text: PackSkip::NotTurnedOn.notice("nda-kit"),
            kind: Some(NoticeKind::SkillPackNotLoaded {
                pack: nda_kit(),
                reason: PackSkipReason::NotTurnedOn,
            }),
        }
    );
}

#[test]
fn two_invalid_kinds_for_one_pack_are_one_notice() {
    let invalid = |refusal| ("nda-kit".to_owned(), PackSkip::Load(Skip::Refused(refusal)));
    let mut notices = PackNotices {
        pending: vec![invalid(Refusal::Script)],
        ..PackNotices::default()
    };
    let told = PackSkip::Load(Skip::Refused(Refusal::Script)).host_notice(nda_kit());
    assert_eq!(notices.untold(), [told]);
    // Another invalid kind reads the same, so the thread is not told again.
    notices.pending = vec![invalid(Refusal::Unlisted), invalid(Refusal::TooBig)];
    assert_eq!(notices.untold(), Vec::<Notice>::new());
    // A missing pack is another reason: told once.
    notices.pending = vec![invalid(Refusal::Missing)];
    assert_eq!(notices.untold().len(), 1);
}

/// A fresh folder under the temp folder for `test`, holding the pack `nda-kit` with one skill.
fn with_pack(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("clauseo-packs-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    let pack = root.join("skill-packs").join("nda-kit");
    let manifest = serde_json::json!({"format": 1, "id": "nda-kit", "name": "A pack",
        "version": "1.0.0", "author": "Example Chambers", "source_url": "https://example.org/pack",
        "licence": "CC-BY-4.0", "last_reviewed": "2026-10-20", "instruction_only": true,
        "skills": [{"name": "nda-review"}]});
    write::create_dir_all(&pack.join("skills").join("nda-review")).unwrap();
    write::write_file(
        &pack.join("clauseo-pack.json"),
        manifest.to_string().as_bytes(),
    )
    .unwrap();
    write::write_file(
        &pack.join("skills").join("nda-review").join("SKILL.md"),
        b"---\nname: nda-review\ndescription: Reviews an NDA.\n---\n# NDA\n",
    )
    .unwrap();
    root
}

/// The profile under `root`.
fn profile(root: &Path) -> ClauseoProfile {
    let paths = ProfilePaths::new(
        &Locations::under_root(root),
        root.join("app").join("clauseo"),
        root.join("stetdoc").join("stetdoc"),
        BrowserSidecars {
            clauseo_mcp: root.join("app").join("clauseo-mcp"),
            agent_browser: root.join("app").join("agent-browser"),
        },
    )
    .unwrap();
    ClauseoProfile::new(paths, None)
}

/// The prep of `nda-kit` with its hash now, as the actor would build it.
fn prep(root: &Path) -> PackPrep {
    let pack =
        clauseo_profile::packs::inspect(&root.join("skill-packs"), "nda-kit", false).unwrap();
    let entries = vec![PackEntry {
        id: "nda-kit".to_owned(),
        hash: pack.hash().clone(),
    }];
    PackPrep {
        write: profile(root).pack_write(Some(AgentKind::Claude), entries, false),
        skipped: vec![("wills-kit".to_owned(), PackSkip::NotTurnedOn)],
    }
}

#[test]
fn the_prep_writes_the_launch_plugin_and_keeps_the_skips() {
    let root = with_pack("prep-writes");
    let plugin = root.join("run").join("s-1").join("skill-packs");
    let (written, skipped) = prep(&root).run(&plugin);
    let written = written.expect("a token");
    assert_eq!(written.plugin(), plugin);
    assert_eq!(written.skills()[0].name, "nda-kit--nda-review");
    assert!(
        plugin
            .join("skills")
            .join("nda-kit--nda-review")
            .join("SKILL.md")
            .is_file()
    );
    assert_eq!(skipped, [("wills-kit".to_owned(), PackSkip::NotTurnedOn)]);
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn a_failed_pack_write_is_a_warning_and_the_launch_goes_on() {
    let root = with_pack("prep-fails");
    // A file where the launch's plugin goes: `write_packs` refuses to write there.
    let plugin = root.join("run").join("s-1").join("skill-packs");
    write::create_dir_all(plugin.parent().unwrap()).unwrap();
    write::write_file(&plugin, b"in the way").unwrap();
    // No error to fail the launch with: no token (so no plugin entry and no index lines), and
    // the skips still reported.
    let (written, skipped) = prep(&root).run(&plugin);
    assert!(written.is_none());
    assert_eq!(skipped, [("wills-kit".to_owned(), PackSkip::NotTurnedOn)]);
    assert_eq!(std::fs::read(&plugin).unwrap(), b"in the way");
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn a_changed_pack_is_skipped_as_changed_and_nothing_is_written() {
    let root = with_pack("prep-changed");
    let prep = prep(&root);
    let readme = root.join("skill-packs").join("nda-kit").join("README.md");
    write::write_file(&readme, b"# Changed\n").unwrap();
    let plugin = root.join("run").join("s-1").join("skill-packs");
    let (written, skipped) = prep.run(&plugin);
    assert!(written.is_none());
    assert!(!plugin.exists());
    assert_eq!(
        skipped,
        [
            ("wills-kit".to_owned(), PackSkip::NotTurnedOn),
            ("nda-kit".to_owned(), PackSkip::Load(Skip::Changed)),
        ]
    );
    write::remove_dir_all(&root).unwrap();
}
