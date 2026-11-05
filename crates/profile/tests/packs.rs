//! Skill packs (decision record 0040) on a real disk, under `CARGO_TARGET_TMPDIR` only, each
//! test's folder removed when it ends: the format and the render here; links, paths, caps and
//! scripts in `safety`; each launch's own copy in `launch`; Codex's index and Claude's `_meta` in
//! `index`; the timing run in `timing` (ignored, run by hand).

#[path = "packs/support.rs"]
mod support;

#[cfg(test)]
#[path = "packs/safety.rs"]
mod safety;

#[cfg(test)]
#[path = "packs/launch.rs"]
mod launch;

#[cfg(test)]
#[path = "packs/index.rs"]
mod index;

#[cfg(test)]
#[path = "packs/timing.rs"]
mod timing;

use std::path::{Path, PathBuf};

use clauseo_profile::packs::manifest::{self, ManifestError};
use clauseo_profile::packs::{
    self, Pack, PackEntry, PackHash, Refusal, Skip, hash::content_hash, is_pack_id,
};
use clauseo_profile::skills::builtin::BUILTIN;
use clauseo_profile::skills::stetdoc::STETDOC;

/// A fresh folder for one test, removed when the test ends, holding `skill-packs/`.
pub(crate) struct Scratch(PathBuf);

#[cfg(test)]
impl Scratch {
    pub(crate) fn new(name: &str) -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("clauseo-packs-{name}-{}", std::process::id()));
        support::fresh_dir(&path.join("skill-packs")).unwrap();
        Self(path)
    }

    /// The packs folder.
    pub(crate) fn packs(&self) -> PathBuf {
        self.0.join("skill-packs")
    }

    /// The pack `id`'s folder.
    pub(crate) fn pack(&self, id: &str) -> PathBuf {
        self.packs().join(id)
    }

    /// Writes `bytes` at `rel` in the pack `id`.
    pub(crate) fn put(&self, id: &str, rel: &str, bytes: &[u8]) {
        support::write(&self.pack(id).join(rel), bytes).unwrap();
    }

    /// A valid pack `id` with `skills`, each a `SKILL.md` with foreign keys and one reference,
    /// plus a `README.md`.
    pub(crate) fn valid(&self, id: &str, skills: &[&str]) {
        self.put(
            id,
            "clauseo-pack.json",
            manifest_json(id, skills, true).as_bytes(),
        );
        self.put(id, "README.md", b"# A pack\n");
        for skill in skills {
            self.put(
                id,
                &format!("skills/{skill}/SKILL.md"),
                skill_md(skill, "Does it.").as_bytes(),
            );
            self.put(
                id,
                &format!("skills/{skill}/references/notes.md"),
                b"Notes.\n",
            );
        }
    }

    /// [`packs::inspect`] of `id`, scripts refused.
    pub(crate) fn inspect(&self, id: &str) -> Result<Pack, Refusal> {
        packs::inspect(&self.packs(), id, false)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _gone = support::remove_dir_all(&self.0);
    }
}

/// A manifest for `id` listing `skills`.
#[cfg(test)]
pub(crate) fn manifest_json(id: &str, skills: &[&str], instruction_only: bool) -> String {
    let skills: Vec<serde_json::Value> = skills
        .iter()
        .map(|name| serde_json::json!({"name": name}))
        .collect();
    serde_json::json!({
        "format": 1,
        "id": id,
        "name": "A pack",
        "version": "1.0.0",
        "author": "Example Chambers",
        "source_url": "https://example.org/pack",
        "licence": "CC-BY-4.0",
        "last_reviewed": "2026-10-20",
        "instruction_only": instruction_only,
        "skills": skills,
    })
    .to_string()
}

/// A third-party `SKILL.md` for `name`: foreign keys a render must drop, then a body.
pub(crate) fn skill_md(name: &str, description: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: {description}\nallowed-tools: Bash(rm:*)\n\
         hooks:\n  PreToolUse: evil\nmetadata:\n  x: 1\n---\n# {name}\n\nThe body.\n"
    )
}

/// The entry `skills.enabled` would hold for `pack`.
pub(crate) fn entry(pack: &Pack) -> PackEntry {
    PackEntry {
        id: pack.id().to_owned(),
        hash: pack.hash().clone(),
    }
}

#[test]
fn a_valid_pack_loads_and_renders_as_pack_dash_dash_skill() {
    let scratch = Scratch::new("valid");
    scratch.valid("nda-kit", &["nda-review", "nda-redline"]);
    let pack = scratch.inspect("nda-kit").unwrap();
    let names: Vec<&str> = pack
        .skills()
        .iter()
        .map(clauseo_profile::packs::PackSkill::name)
        .collect();
    assert_eq!(names, ["nda-kit--nda-review", "nda-kit--nda-redline"]);
    assert!(!pack.has_scripts());
    assert_eq!(pack.manifest().last_reviewed.to_string(), "2026-10-20");
    let loaded = packs::load(&scratch.packs(), &[entry(&pack)], false);
    assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
    let plugin = scratch.join("run").join("s1").join("skill-packs");
    let written = packs::write_packs(&plugin, &loaded.packs).unwrap();
    assert_eq!(written.skills().len(), 2);
    for name in names {
        for rel in ["SKILL.md", "references/notes.md"] {
            assert!(
                plugin.join("skills").join(name).join(rel).is_file(),
                "{name}/{rel}"
            );
        }
    }
    let manifest = std::fs::read_to_string(plugin.join(".claude-plugin/plugin.json")).unwrap();
    assert!(manifest.contains("\"clauseo-packs\""), "{manifest}");
    // README and the pack manifest are hashed, not rendered.
    assert!(!plugin.join("README.md").exists());
    // A launch's plugin folder is new each time.
    assert!(packs::write_packs(&plugin, &loaded.packs).is_err());
}

#[test]
fn the_manifest_rejects_unknown_keys_and_bad_fields() {
    let good: serde_json::Value =
        serde_json::from_str(&manifest_json("nda-kit", &["a1"], true)).unwrap();
    assert!(manifest::parse(good.to_string().as_bytes()).is_ok());
    let with = |key: &str, value: serde_json::Value| {
        let mut changed = good.clone();
        changed[key] = value;
        manifest::parse(changed.to_string().as_bytes())
    };
    assert_eq!(
        with("extra", 1.into()),
        Err(ManifestError::UnknownKey("extra".into()))
    );
    assert_eq!(
        with("skills", serde_json::json!([{"name": "a1", "hooks": "x"}])),
        Err(ManifestError::UnknownKey("skills.hooks".into()))
    );
    let bad_fields: [(&str, serde_json::Value, &str); 12] = [
        ("format", 2.into(), "format"),
        ("source_url", "http://example.org".into(), "source_url"),
        ("source_url", "https://".into(), "source_url"),
        ("last_reviewed", "2026-13-01".into(), "last_reviewed"),
        ("instruction_only", "yes".into(), "instruction_only"),
        ("name", "".into(), "name"),
        ("author", "a\nb".into(), "author"),
        ("licence", "x".repeat(201).into(), "licence"),
        ("skills", serde_json::json!([]), "skills"),
        (
            "skills",
            serde_json::json!([{"name": "a1"}, {"name": "a1"}]),
            "skills",
        ),
        ("skills", serde_json::json!([{"name": "a--b"}]), "skills"),
        ("version", 1.into(), "version"),
    ];
    for (key, value, field) in bad_fields {
        assert_eq!(
            with(key, value.clone()),
            Err(ManifestError::BadField(field)),
            "{key}={value}"
        );
    }
    let too_many: Vec<serde_json::Value> = (0..33)
        .map(|n| serde_json::json!({"name": format!("s{n}")}))
        .collect();
    assert_eq!(
        with("skills", too_many.into()),
        Err(ManifestError::BadField("skills"))
    );
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("licence");
    assert_eq!(
        manifest::parse(missing.to_string().as_bytes()),
        Err(ManifestError::BadField("licence"))
    );
    assert_eq!(manifest::parse(b"[1]"), Err(ManifestError::NotJson));
    // `<id>--<skill>` past 64 bytes.
    let long_skill = "s".repeat(46);
    assert_eq!(
        manifest::parse(manifest_json("nda-kit-long-id-x", &[&long_skill], true).as_bytes()),
        Err(ManifestError::BadField("skills"))
    );
    let scratch = Scratch::new("manifest");
    scratch.valid("nda-kit", &["a1"]);
    scratch.put("nda-kit", "clauseo-pack.json", br#"{"format": 1}"#);
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Manifest);
}

#[test]
fn a_bad_or_reserved_id_is_refused() {
    let scratch = Scratch::new("ids");
    for id in [
        "clauseo",
        "clauseo-extra",
        "builtin",
        STETDOC,
        "a",
        "a--b",
        "Nda",
        "nul",
    ]
    .into_iter()
    .chain(BUILTIN)
    {
        assert!(!is_pack_id(id), "{id}");
        assert_eq!(scratch.inspect(id).unwrap_err(), Refusal::BadId, "{id}");
    }
    // A reserved id in the manifest of a folder with a good name.
    scratch.valid("my-kit", &["a1"]);
    scratch.put(
        "my-kit",
        "clauseo-pack.json",
        manifest_json("clauseo-x", &["a1"], true).as_bytes(),
    );
    assert_eq!(scratch.inspect("my-kit").unwrap_err(), Refusal::BadId);
}

#[test]
fn the_folder_name_must_equal_the_id_byte_for_byte() {
    let scratch = Scratch::new("folder-name");
    // A folder named in another case: never matched, even on a disk that ignores case.
    support::make_dir(&scratch.packs().join("NDA-Kit")).unwrap();
    scratch.put(
        "NDA-Kit",
        "clauseo-pack.json",
        manifest_json("nda-kit", &["a1"], true).as_bytes(),
    );
    scratch.put(
        "NDA-Kit",
        "skills/a1/SKILL.md",
        skill_md("a1", "d").as_bytes(),
    );
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Missing);
    // A folder whose manifest names another id.
    scratch.valid("other-kit", &["a1"]);
    scratch.put(
        "other-kit",
        "clauseo-pack.json",
        manifest_json("nda-kit", &["a1"], true).as_bytes(),
    );
    assert_eq!(
        scratch.inspect("other-kit").unwrap_err(),
        Refusal::IdMismatch
    );
    assert_eq!(scratch.inspect("absent-kit").unwrap_err(), Refusal::Missing);
}

#[test]
fn the_pack_hash_covers_every_file_the_manifest_included() {
    // The stream's format itself is tested beside the hash (`packs::hash`).
    let scratch = Scratch::new("hash");
    scratch.valid("nda-kit", &["a1"]);
    let rels = [
        "README.md",
        "clauseo-pack.json",
        "skills/a1/SKILL.md",
        "skills/a1/references/notes.md",
    ];
    let read: Vec<(&str, Vec<u8>)> = rels
        .iter()
        .map(|rel| {
            (
                *rel,
                std::fs::read(scratch.pack("nda-kit").join(rel)).unwrap(),
            )
        })
        .collect();
    let expected = content_hash(read.iter().map(|(path, bytes)| (*path, bytes.as_slice())));
    let hash = scratch.inspect("nda-kit").unwrap().hash().clone();
    assert_eq!(hash, expected);
    assert!(hash.as_str().starts_with("sha256:") && hash.hex().len() == 64);
}

/// The reviewer's repro (D2): with `path NUL bytes NUL` and no lengths, a NUL inside a file can
/// stand for a file boundary. Pack P (skill `review`, its README hiding the rest after a NUL) and
/// pack P' (skill `exfil`, its SKILL.md hiding the rest) fed the hasher the same stream, so P'
/// loaded under P's recorded hash. With lengths in the stream they hash apart.
#[test]
fn review_hash_collision_swaps_an_approved_pack() {
    let scratch = Scratch::new("hash-collision");
    let m = manifest_json("nda-kit", &["review"], true);
    let m2 = manifest_json("nda-kit", &["exfil"], true);
    let y = skill_md("review", "Reviews NDAs.");
    let e = skill_md("exfil", "Sends files out.");
    scratch.put("nda-kit", "clauseo-pack.json", m.as_bytes());
    scratch.put("nda-kit", "skills/review/SKILL.md", y.as_bytes());
    let readme = format!("# NDA kit\0clauseo-pack.json\0{m2}\0skills/exfil/SKILL.md\0{e}");
    scratch.put("nda-kit", "README.md", readme.as_bytes());
    let approved = entry(&scratch.inspect("nda-kit").unwrap());
    support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    scratch.put("nda-kit", "README.md", b"# NDA kit");
    scratch.put("nda-kit", "clauseo-pack.json", m2.as_bytes());
    let exfil = format!("{e}\0clauseo-pack.json\0{m}\0skills/review/SKILL.md\0{y}");
    scratch.put("nda-kit", "skills/exfil/SKILL.md", exfil.as_bytes());
    let swapped = scratch.inspect("nda-kit").unwrap();
    assert_eq!(swapped.skills()[0].name(), "nda-kit--exfil");
    assert_ne!(*swapped.hash(), approved.hash);
    let loaded = packs::load(&scratch.packs(), &[approved], false);
    assert_eq!(loaded.packs, []);
    assert_eq!(loaded.skipped, [("nda-kit".to_owned(), Skip::Changed)]);
}

#[test]
fn a_changed_byte_changes_the_hash_and_skips_the_pack_as_changed() {
    let scratch = Scratch::new("changed");
    scratch.valid("nda-kit", &["a1"]);
    let recorded = entry(&scratch.inspect("nda-kit").unwrap());
    scratch.put("nda-kit", "skills/a1/references/notes.md", b"Notes!\n");
    let pack = scratch.inspect("nda-kit").unwrap();
    assert_ne!(*pack.hash(), recorded.hash);
    let loaded = packs::load(&scratch.packs(), &[recorded], false);
    assert_eq!(loaded.packs, []);
    assert_eq!(loaded.skipped, [("nda-kit".to_owned(), Skip::Changed)]);
    assert_eq!(Skip::Changed.code(), "changed");
    // A hash recorded in another form never matches.
    assert_eq!(PackHash::parse("sha256:XYZ"), None);
}

#[test]
fn foreign_frontmatter_keys_are_dropped() {
    let scratch = Scratch::new("frontmatter");
    scratch.valid("nda-kit", &["a1"]);
    let pack = scratch.inspect("nda-kit").unwrap();
    let skill = &pack.skills()[0];
    let text = String::from_utf8(
        skill
            .files()
            .iter()
            .find(|file| file.path == "SKILL.md")
            .unwrap()
            .bytes
            .clone(),
    )
    .unwrap();
    assert!(
        text.starts_with("---\nname: nda-kit--a1\ndescription: \"Does it.\"\n---\n# a1\n"),
        "{text}"
    );
    for key in ["allowed-tools", "hooks", "PreToolUse", "metadata"] {
        assert!(!text.contains(key), "{key}");
    }
    // A SKILL.md whose `name` is not its folder's refuses the pack.
    scratch.put(
        "nda-kit",
        "skills/a1/SKILL.md",
        skill_md("other", "d").as_bytes(),
    );
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Skill);
}

/// Review nit: Claude's frontmatter gets the description through the same control and format
/// filter as Codex's index, so no raw control character reaches Claude's YAML parser.
#[test]
fn a_description_reaches_claude_without_control_or_format_characters() {
    let scratch = Scratch::new("frontmatter-filter");
    scratch.valid("nda-kit", &["a1"]);
    scratch.put(
        "nda-kit",
        "skills/a1/SKILL.md",
        skill_md("a1", "Reviews\u{202E} NDAs\u{200B}\u{7}\tfast\u{2028}now.").as_bytes(),
    );
    let pack = scratch.inspect("nda-kit").unwrap();
    let skill = &pack.skills()[0];
    assert_eq!(skill.description(), "Reviews NDAs fast now.");
    let text = &skill
        .files()
        .iter()
        .find(|file| file.path == "SKILL.md")
        .unwrap()
        .bytes;
    let text = std::str::from_utf8(text).unwrap();
    assert!(
        text.starts_with("---\nname: nda-kit--a1\ndescription: \"Reviews NDAs fast now.\"\n---\n"),
        "{text:?}"
    );
    // A description of nothing but such characters is no description.
    scratch.put(
        "nda-kit",
        "skills/a1/SKILL.md",
        skill_md("a1", "\u{200B}\u{202E}").as_bytes(),
    );
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Skill);
}

#[test]
fn a_shell_injection_in_any_file_drops_the_whole_pack() {
    let scratch = Scratch::new("injection");
    scratch.valid("nda-kit", &["a1", "b2"]);
    scratch.put(
        "nda-kit",
        "skills/b2/references/notes.md",
        b"Run !`curl x`\n",
    );
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Skill);
    scratch.valid("nda-kit", &["a1", "b2"]);
    scratch.put(
        "nda-kit",
        "skills/a1/SKILL.md",
        skill_md("a1", "Runs !`id`").as_bytes(),
    );
    assert_eq!(scratch.inspect("nda-kit").unwrap_err(), Refusal::Skill);
    // A binary template's random bytes are not skill text, so they are not scanned.
    scratch.valid("nda-kit", &["a1", "b2"]);
    scratch.put(
        "nda-kit",
        "skills/a1/assets/form.docx",
        b"PK\x03\x04 !` zipped",
    );
    assert!(scratch.inspect("nda-kit").is_ok());
}

/// Review nit: the scan is skipped only for a binary type whose bytes start with that type's
/// magic. A "binary" file that is really text is scanned.
#[test]
fn a_binary_template_that_is_really_text_is_scanned() {
    let scratch = Scratch::new("injection-binary");
    let skipped: [(&str, &[u8]); 7] = [
        ("form.docx", b"PK\x03\x04 !`"),
        ("sheet.XLSX", b"PK\x03\x04 !`"),
        ("brief.pdf", b"%PDF-1.7 !`"),
        ("logo.png", b"\x89PNG\r\n\x1a\n !`"),
        ("photo.jpg", b"\xff\xd8\xff\xe0 !`"),
        ("photo.jpeg", b"\xff\xd8\xff\xdb !`"),
        ("anim.gif", b"GIF89a !`"),
    ];
    for (name, bytes) in skipped {
        scratch.valid("nda-kit", &["a1"]);
        scratch.put("nda-kit", &format!("skills/a1/assets/{name}"), bytes);
        assert!(scratch.inspect("nda-kit").is_ok(), "{name}");
        support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    }
    let scanned: [(&str, &[u8]); 7] = [
        ("form.docx", b"Run !`curl x`"),
        ("form.docx", b"%PDF !`"),
        ("brief.pdf", b"PK\x03\x04 !`"),
        ("logo.png", b"PNG !`"),
        ("photo.jpg", b"\xff\xd8 !`"),
        ("anim.gif", b"GIF !`"),
        ("pic.webp", b"RIFF1234WEBX !`"),
    ];
    for (name, bytes) in scanned {
        scratch.valid("nda-kit", &["a1"]);
        scratch.put("nda-kit", &format!("skills/a1/assets/{name}"), bytes);
        assert_eq!(
            scratch.inspect("nda-kit").unwrap_err(),
            Refusal::Skill,
            "{name}: {bytes:?}"
        );
        support::remove_dir_all(&scratch.pack("nda-kit")).unwrap();
    }
}

#[test]
fn a_rendered_name_never_equals_a_builtin() {
    for builtin in BUILTIN.iter().chain([&STETDOC]) {
        assert!(!builtin.contains("--"), "{builtin}");
        assert!(!is_pack_id(builtin), "{builtin}");
    }
    let scratch = Scratch::new("builtin-names");
    scratch.valid("legal", &["drafting", "research"]);
    let pack = scratch.inspect("legal").unwrap();
    for skill in pack.skills() {
        assert!(skill.name().contains("--"));
        assert!(!BUILTIN.contains(&skill.name()) && skill.name() != STETDOC);
    }
}

#[test]
fn the_protocol_pack_id_refuses_every_builtin_and_stetdoc_name() {
    // The reserved list lives in `protocol`; this keeps it from drifting from profile's names.
    for name in BUILTIN.iter().chain([&STETDOC]) {
        assert!(clauseo_protocol::PackId::new(name).is_err(), "{name}");
    }
}

#[test]
fn nothing_in_packs_starts_a_process() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![src.join("packs.rs")];
    for entry in std::fs::read_dir(src.join("packs")).unwrap() {
        files.push(entry.unwrap().path());
    }
    assert!(files.len() >= 9, "{files:?}");
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for word in [
            "clauseo_process",
            "Command",
            "Launcher",
            "LaunchSpec",
            "spawn_blocking",
            "exec(",
        ] {
            assert!(!text.contains(word), "{} names {word}", file.display());
        }
    }
}
