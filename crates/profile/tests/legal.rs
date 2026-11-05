//! R22's text, read as shipped: the legal part of the instructions and the three built-in
//! skills' source files, their budgets, the words they must never hold, the version lock, and
//! the skills index Codex gets. Nothing is written; the lock test reads `assets/skills/`.

#[path = "legal/index.rs"]
mod index;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clauseo_profile::RenderedSkill;
use clauseo_profile::instructions::{LEGAL, TOOLS, word_count};
use clauseo_profile::placeholders::{CONNECTORS, IN_INSTRUCTIONS, MATTER_NOTES, WORK_FOLDER};
use clauseo_profile::skills::builtin::{ASSETS, BUILTIN, RETIRED_BUILTINS, sources};
use clauseo_profile::skills::frontmatter::{self, MetaValue};
use clauseo_profile::skills::stetdoc::STETDOC;
use clauseo_profile::skills::{SKILL_MD, is_skill_name, render};
use clauseo_profile::{BrowserSidecars, ClauseoProfile, Instructions, ProfilePaths};
use clauseo_settings::Locations;
use sha2::{Digest as _, Sha256};

/// The ten headings of the legal part, in order (R22, "The instructions").
const HEADINGS: [&str; 10] = [
    "Identity and audience",
    "Intake",
    "Evidence",
    "Citations",
    "Working notes",
    "Review before delivery",
    "Matter brief upkeep",
    "Conduct",
    "How turns end",
    "Untrusted content",
];

/// v2's tool names (R22, "Tool names"): whole words, case-sensitive.
const V2_WORDS: [&str; 30] = [
    "writeFile",
    "readFile",
    "editFile",
    "listFiles",
    "globFiles",
    "grepFiles",
    "viewFile",
    "promoteDeliverable",
    "acknowledgeProblems",
    "delegate",
    "loadSkill",
    "code_execution",
    "flashLite",
    "updateTodos",
    "searchLegalCases",
    "searchInDocument",
    "getLegalDocument",
    "readAround",
    "quoteCheck",
    "queryCCI",
    "createWatch",
    "listWatches",
    "updateWatch",
    "linkMatter",
    "check_citations",
    "citationReport",
    "coverageReport",
    "registerParse",
    "corgi",
    "bash",
];

/// v2's names that match as substrings.
const V2_SUBSTRINGS: [&str; 4] = ["python-docx", "lxml", "toolout/", "workproduct/"];

/// `path` made absolute on this OS: as it is on Unix, on drive `C:` on Windows.
fn abs(path: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!("C:{path}"))
    } else {
        PathBuf::from(path)
    }
}

#[cfg(test)]
fn paths_under(root: &Path) -> ProfilePaths {
    ProfilePaths::new(
        &Locations::under_root(root),
        abs("/app/clauseo"),
        abs("/app/stetdoc/stetdoc"),
        BrowserSidecars {
            clauseo_mcp: abs("/app/clauseo-mcp"),
            agent_browser: abs("/app/agent-browser"),
        },
    )
    .unwrap()
}

/// The launch profile with the three built-ins and a stetdoc skill, as desktop builds it.
#[cfg(test)]
fn launch_profile(root: &Path) -> ClauseoProfile {
    let stetdoc = clauseo_profile::skills::stetdoc::skill_source(
        "stetdoc 0.9.1",
        b"---\ndescription: Edit Word files as tracked changes.\n---\n# stetdoc\n",
    )
    .unwrap();
    let (profile, refused) =
        ClauseoProfile::new(paths_under(root), None).with_builtin_skills(Some(stetdoc));
    assert!(refused.is_empty(), "{refused:?}");
    profile
}

/// Every shipped text by a name: the legal part, then each skill source file.
fn shipped<'a>() -> Vec<(String, &'a str)> {
    std::iter::once(("legal.md".to_owned(), LEGAL))
        .chain(
            ASSETS
                .iter()
                .map(|(skill, path, text)| (format!("{skill}/{path}"), *text)),
        )
        .collect()
}

/// The `SKILL.md` of each built-in, by name.
fn skill_mds() -> Vec<(&'static str, &'static str)> {
    ASSETS
        .iter()
        .filter(|(_, path, _)| *path == SKILL_MD)
        .map(|(skill, _, text)| (*skill, *text))
        .collect()
}

#[test]
fn legal_word_budget() {
    let legal = word_count(LEGAL);
    assert!(legal <= 1200, "the legal part has {legal} words");
    let whole = ClauseoProfile::new(paths_under(Path::new("/data")), None).instructions();
    assert_eq!(
        whole,
        Instructions::tools()
            .with_connectors()
            .with_part(LEGAL)
            .compose("")
    );
    assert!(whole.ends_with(&format!("\n\n{}", LEGAL.trim())));
    assert_eq!(
        word_count(&whole),
        word_count(TOOLS) + word_count(LEGAL),
        "only {{author}} is filled, with one word"
    );
    assert!(word_count(&whole) <= 1500, "{} words", word_count(&whole));
}

#[test]
fn legal_sections_in_order() {
    let found: Vec<&str> = LEGAL
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .collect();
    assert_eq!(found, HEADINGS);
    assert!(!LEGAL.lines().any(|line| line.starts_with("# ")));
}

#[test]
fn legal_names_matter_brief() {
    assert!(LEGAL.contains("{matter_notes}/matter-brief.md"));
    assert!(LEGAL.contains("{work_folder}/notes.md"));
    assert!(LEGAL.contains("{work_folder}/figures.md"));
    assert!(!LEGAL.contains("{work_folder}/matter-brief.md"));
}

#[test]
fn the_launch_instructions_hold_only_tokens_the_host_fills() {
    let text = ClauseoProfile::new(paths_under(Path::new("/data")), None).instructions();
    assert!(text.contains(WORK_FOLDER) && text.contains(MATTER_NOTES));
    assert!(!text.contains("{author}"));
    let filled = clauseo_profile::placeholders::fill(
        &text,
        &IN_INSTRUCTIONS,
        &[(WORK_FOLDER, "W"), (MATTER_NOTES, "M"), (CONNECTORS, "")],
    )
    .unwrap();
    assert!(!filled.contains("{work_folder}") && !filled.contains("{matter_notes}"));
    assert!(!filled.contains(CONNECTORS));
}

/// R24's connectors paragraph (decision D6): at most 80 words, between the tools part and R22's
/// legal part when the session got the `case-law` entry; otherwise the same place says there is
/// no case-law database. The two never meet: the legal part and the skills say neither outright.
#[test]
fn the_connectors_paragraph_goes_only_where_the_host_puts_it() {
    use clauseo_profile::instructions::{
        CONNECTORS as PARAGRAPH, CONNECTORS_MAX_WORDS, CONNECTORS_NONE, TOOLS, connectors_part,
        word_count,
    };
    const DENIAL: &str = "no case-law database";
    assert!(
        word_count(PARAGRAPH) <= CONNECTORS_MAX_WORDS,
        "{} words",
        word_count(PARAGRAPH)
    );
    insta::assert_snapshot!(PARAGRAPH.trim());
    let text = ClauseoProfile::new(paths_under(Path::new("/data")), None).instructions();
    assert_eq!(text.matches(CONNECTORS).count(), 1);
    assert!(
        !text.contains(DENIAL),
        "the composed text denies case law outright"
    );
    let fill = |with: bool| {
        let part = connectors_part(with);
        clauseo_profile::placeholders::fill(
            &text,
            &IN_INSTRUCTIONS,
            &[(WORK_FOLDER, "W"), (MATTER_NOTES, "M"), (CONNECTORS, part)],
        )
        .unwrap()
    };
    let (with, without) = (fill(true), fill(false));
    assert!(!with.contains(DENIAL), "{with}");
    assert!(without.contains(DENIAL), "{without}");
    assert!(!without.contains("case-law tools"));
    let tools = TOOLS
        .trim()
        .replace("{author}", "Clauseo")
        .replace("{work_folder}", "W");
    for (filled, part) in [(&with, PARAGRAPH), (&without, CONNECTORS_NONE)] {
        let expected = format!("{tools}\n\n{}\n\n## Identity", part.trim());
        assert!(filled.starts_with(&expected), "{filled}");
    }
    // The research skill's line is conditional: no outright denial for the agent to obey.
    let research = skill_mds()
        .into_iter()
        .find(|(name, _)| *name == "legal-research")
        .unwrap()
        .1;
    assert!(research.contains("If case-law tools are available in this session"));
    assert!(!research.contains("You have no case-law database"));
}

#[test]
fn skill_frontmatter_valid() {
    let mut names = BTreeSet::from([STETDOC.to_owned()]);
    for (folder, text) in skill_mds() {
        let (front, _) = frontmatter::parse(text).unwrap();
        assert!(
            is_skill_name(&front.name)
                && front.name.len() <= 64
                && front
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "{}",
            front.name
        );
        assert_eq!(front.name, folder);
        assert!(names.insert(front.name.clone()), "{} twice", front.name);
        let chars = front.description.chars().count();
        assert!((20..=1024).contains(&chars), "{folder}: {chars} characters");
        assert!(!front.description.contains(['\n', '"', ':']), "{folder}");
        for key in ["version", "author", "tools", "tier"] {
            assert!(front.clauseo.contains_key(key), "{folder}: no {key}");
        }
        assert!(
            matches!(front.clauseo["tools"], MetaValue::List(_)),
            "{folder}: tools is not an inline list"
        );
    }
    assert_eq!(names.len(), 4);
    assert_eq!(skill_mds().len(), BUILTIN.len());
}

#[test]
fn the_rendered_frontmatter_is_name_and_description_alone() {
    let rendered = render(&sources().unwrap()).unwrap();
    let names: Vec<&str> = rendered.skills().iter().map(RenderedSkill::name).collect();
    assert_eq!(names, BUILTIN);
    for skill in rendered.skills() {
        let md = skill.files().iter().find(|f| f.path == SKILL_MD).unwrap();
        let text = std::str::from_utf8(&md.bytes).unwrap();
        let (front, _) = frontmatter::parse(text).unwrap();
        assert!(front.clauseo.is_empty());
        assert!(text.starts_with(&frontmatter::render(skill.name(), &front.description)));
        assert!(!text.contains("metadata:"));
        // R24 gave the research skill's case-law line conditional wording: version 2.
        let version = if skill.name() == "legal-research" {
            "2"
        } else {
            "1"
        };
        assert_eq!(skill.entry().version, version, "{}", skill.name());
    }
}

#[test]
fn skill_body_budget() {
    for (name, text) in skill_mds() {
        let (_, body) = frontmatter::parse(text).unwrap();
        assert!(
            word_count(body) < 5000,
            "{name}: {} words",
            word_count(body)
        );
    }
    for (skill, path, text) in ASSETS {
        if path.starts_with("references/") {
            assert!(word_count(text) <= 1000, "{skill}/{path}");
        }
    }
}

/// The words of `text`: runs of ASCII letters, digits and `_`.
fn words(text: &str) -> BTreeSet<&str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .collect()
}

/// The v2 names `text` holds.
fn v2_names(text: &str) -> Vec<&'static str> {
    let held = words(text);
    V2_WORDS
        .iter()
        .filter(|name| held.contains(*name))
        .chain(V2_SUBSTRINGS.iter().filter(|name| text.contains(*name)))
        .copied()
        .collect()
}

#[test]
fn no_v2_tool_names() {
    assert_eq!(v2_names("call writeFile, then bash"), ["writeFile", "bash"]);
    assert_eq!(v2_names("Bash and delegated work"), Vec::<&str>::new());
    assert_eq!(v2_names("unzip into workproduct/x"), ["workproduct/"]);
    let index = launch_profile(Path::new("/data")).skills_index();
    for (name, text) in shipped()
        .into_iter()
        .chain([("index".to_owned(), index.as_str())])
    {
        assert!(v2_names(text).is_empty(), "{name}: {:?}", v2_names(text));
    }
}

#[test]
fn no_ik_or_viewer_links() {
    for (name, text) in shipped() {
        assert!(!text.contains("indiankanoon.org"), "{name}");
        assert!(!text.contains("clauseo.chat"), "{name}");
    }
}

#[test]
fn no_dashes() {
    let index = launch_profile(Path::new("/data")).skills_index();
    for (name, text) in shipped()
        .into_iter()
        .chain([("index".to_owned(), index.as_str())])
    {
        assert!(!text.contains(['\u{2014}', '\u{2013}']), "{name}");
    }
}

#[test]
fn no_placeholder_in_skills() {
    for (skill, path, text) in ASSETS {
        for token in [WORK_FOLDER, MATTER_NOTES, "{author}"] {
            assert!(!text.contains(token), "{skill}/{path} holds {token}");
        }
    }
}

/// The lock's hash of one skill's source folder, as decision record 0031 defines it: every file
/// under the folder sorted by its `/`-separated relative path as bytes; for each, the path, one
/// 0x00, the bytes, one 0x00.
#[cfg(test)]
fn skill_hash(folder: &Path) -> String {
    let mut files = Vec::new();
    collect(folder, folder, &mut files);
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut hasher = Sha256::new();
    for (rel, path) in files {
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        hasher.update(std::fs::read(path).unwrap());
        hasher.update([0]);
    }
    sha256_digest_hex(&hasher.finalize())
}

/// Lower-case hex of `bytes`.
fn sha256_digest_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _infallible = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.push((rel, path));
        }
    }
}

/// The lock's lines as they stand, which the file must start with: a line is never edited or
/// removed, only appended after these. Changing one of these lines means changing this constant,
/// which review sees (decision record 0031). Append a line here when the lock grows.
const LAUNCH_LOCK_LINES: &str = "# Append-only. Each line is <skill> <version> <sha256 of the skill source files>; never edit or remove a line.\n# A text change to a skill adds a new line with a new version (see docs/decisions, legal instructions and skills).\nlegal-drafting 1 2e3f78effb79302213a392d335c6facb1c071e1389b579f7ead9740903c675f6\nlegal-research 1 8c136575eec7b0c2b4b3222b46504417b4182fe7cd9a08c4538c0e506aeb42c2\nmatter-brief 1 d535e938547b992311639fa5c9cbecb27b50c767b94a698565ecc880ab6659e6\n";

#[test]
fn versions_lock_is_append_only() {
    let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/skills");
    let lock = std::fs::read_to_string(skills.join("versions.lock")).unwrap();
    assert!(LAUNCH_LOCK_LINES.ends_with('\n'));
    assert!(
        lock.starts_with(LAUNCH_LOCK_LINES),
        "versions.lock no longer starts with the pinned lines: a line was edited or removed"
    );
}

/// Checks a lock's text against each skill's current `(version, hash)`: no (skill, version)
/// with two hashes, and each current hash listed under the current version.
fn check_lock(lock: &str, current: &[(&str, &str, &str)]) -> Result<(), String> {
    let mut seen: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    let mut lines = BTreeSet::new();
    for line in lock.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(' ').collect();
        let [skill, version, hash] = parts[..] else {
            return Err(format!("`{line}` is not `<skill> <version> <hash>`"));
        };
        if let Some(other) = seen.insert((skill, version), hash)
            && other != hash
        {
            return Err(format!("{skill} {version} has two hashes"));
        }
        lines.insert((skill, version, hash));
    }
    for (skill, version, hash) in current {
        if !lines.contains(&(*skill, *version, *hash)) {
            return Err(format!(
                "{skill}'s current text ({hash}) is not listed under version {version}"
            ));
        }
    }
    Ok(())
}

#[test]
fn versions_lock() {
    let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/skills");
    let lock = std::fs::read_to_string(skills.join("versions.lock")).unwrap();
    let mut current = Vec::new();
    for (name, text) in skill_mds() {
        let (front, _) = frontmatter::parse(text).unwrap();
        let MetaValue::Scalar(version) = front.clauseo["version"].clone() else {
            panic!("{name}: version is not a scalar");
        };
        current.push((name, version, skill_hash(&skills.join(name))));
    }
    let current: Vec<(&str, &str, &str)> = current
        .iter()
        .map(|(name, version, hash)| (*name, version.as_str(), hash.as_str()))
        .collect();
    assert_eq!(check_lock(&lock, &current), Ok(()));

    let (name, version, hash) = current[0];
    let twice = format!("{lock}{name} {version} {}\n", "0".repeat(64));
    assert!(
        check_lock(&twice, &current)
            .unwrap_err()
            .contains("two hashes")
    );
    let changed = [(name, version, "f".repeat(64))];
    let changed: Vec<(&str, &str, &str)> = changed
        .iter()
        .map(|(n, v, h)| (*n, *v, h.as_str()))
        .collect();
    assert!(
        check_lock(&lock, &changed)
            .unwrap_err()
            .contains("not listed")
    );
    let bumped_text = [(name, "2", hash)];
    assert!(
        check_lock(&lock, &bumped_text)
            .unwrap_err()
            .contains("not listed")
    );
}

/// The embedded list is exactly the files under each built-in's source folder, byte for byte.
#[test]
fn the_embedded_assets_are_the_source_folders() {
    let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/skills");
    let mut on_disk = BTreeMap::new();
    for name in BUILTIN {
        let mut files = Vec::new();
        collect(&skills.join(name), &skills.join(name), &mut files);
        for (rel, path) in files {
            on_disk.insert((name.to_owned(), rel), std::fs::read(path).unwrap());
        }
    }
    let embedded: BTreeMap<(String, String), Vec<u8>> = ASSETS
        .iter()
        .map(|(skill, path, text)| {
            (
                ((*skill).to_owned(), (*path).to_owned()),
                text.as_bytes().to_vec(),
            )
        })
        .collect();
    assert_eq!(embedded, on_disk);
}

#[test]
fn no_retired_name_is_a_shipped_skill() {
    for name in RETIRED_BUILTINS {
        assert!(!BUILTIN.contains(&name) && name != STETDOC, "{name}");
    }
}
