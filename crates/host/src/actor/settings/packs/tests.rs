//! The checks `SetSkillPack` makes on the actor, against settings built from a policy's text:
//! who decided a refusal (`Admission`), and the re-check after the read (`recheck`).

use std::path::Path;

use clauseo_protocol::{ErrorCode, PackId};
use clauseo_settings::policy::file;
use clauseo_settings::{Effective, Locations, PolicyRead, PolicySource, SystemFiles};

use super::{Admission, Hashed, recheck};
use crate::pipeline::tests::support::TempDir;

/// A policy source whose `policy.json` text is `text`.
struct Text(&'static str);

impl PolicySource for Text {
    fn name(&self) -> &'static str {
        "/etc/clauseo/policy.json"
    }

    fn read(&self) -> PolicyRead {
        PolicyRead::Read(file::parse(Path::new(self.name()), self.0.as_bytes()).unwrap())
    }
}

/// The settings under `dir` (no `settings.json`) with `policy`.
fn load(dir: &TempDir, policy: &'static str) -> Effective {
    Effective::load(
        &Locations::under_root(&dir.path().join("data")),
        &SystemFiles,
        &[&Text(policy)],
    )
}

fn kit() -> PackId {
    PackId::new("nda-kit").unwrap()
}

fn hashed(scripts: bool) -> Hashed {
    Hashed {
        hash: format!("sha256:{}", "a".repeat(64)),
        scripts,
    }
}

/// A policy that turns `allow_scripts` off while a pack with a script was being read: the
/// re-check refuses it as the read would have, `settings.refused` with `script`.
#[test]
fn the_recheck_refuses_a_pack_with_scripts_once_allow_scripts_is_off() {
    let dir = TempDir::new("packs_recheck_scripts");
    let off = load(
        &dir,
        r#"{"skills": {"extra": "any", "allow_scripts": false}}"#,
    );
    let error = recheck(&off, &kit(), &hashed(true)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::SettingsRefused, "{error}");
    assert!(error.to_string().contains("(script)"), "{error}");
    assert!(recheck(&off, &kit(), &hashed(false)).is_ok());
    let on = load(
        &dir,
        r#"{"skills": {"extra": "any", "allow_scripts": true}}"#,
    );
    assert!(recheck(&on, &kit(), &hashed(true)).is_ok());
}

/// `off` is the firm's only when policy locks `skills.extra`; a firm's `skills.curated` alone
/// does not make the lawyer's own `off` the firm's. `curated` leaving an id out is the firm's.
#[test]
fn by_policy_is_decided_per_branch() {
    let dir = TempDir::new("packs_by_policy");
    let curated_only = load(&dir, r#"{"skills": {"curated": ["lease-kit"]}}"#);
    // No settings.json: `extra` is the lawyer's default, `off`.
    assert_eq!(Admission::of(&curated_only).refusal("nda-kit"), Some(false));
    let extra_off = load(&dir, r#"{"skills": {"extra": "off"}}"#);
    assert_eq!(Admission::of(&extra_off).refusal("nda-kit"), Some(true));
    let curated = load(
        &dir,
        r#"{"skills": {"extra": "curated", "curated": ["lease-kit"]}}"#,
    );
    assert_eq!(Admission::of(&curated).refusal("nda-kit"), Some(true));
    assert_eq!(Admission::of(&curated).refusal("lease-kit"), None);
}
