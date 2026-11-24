//! Adding skills from where a lawyer finds them (decision record 0040, "Adding from a link"):
//! `SearchSkills`, `PreviewSkill` and `AddSkills` against a fake server on 127.0.0.1
//! (`skill_server.rs`), never the network: the search and its retry, a GitHub repository's
//! preview, adding one of its skills and the list's `from`, `has_scripts` and `icon`, an icon
//! over its cap, an expired preview, a too-big archive, a 404, and a skill added for one matter.

use std::time::Duration;

use clauseo_protocol::{
    AddonFromKind, AddonKind, AddonScope, ErrorCode, HostCommand, HostQuery, MatterId, QueryResult,
    SkillFrom, SkillPreview,
};

use crate::addons::scope::{row, two_matters};
use crate::addons::{addons, item};
use crate::skill_server::{SkillServer, zip_of};
use crate::support::{Dir, Running, foreign_uuid};

/// A `SKILL.md` named `name`.
fn skill_md(name: &str) -> Vec<u8> {
    format!("---\nname: {name}\ndescription: Does {name} work.\n---\n# {name}\n").into_bytes()
}

/// GitHub's archive of a repository with two skills, `pdf` with a script and `docx` without.
fn repo_archive() -> Vec<u8> {
    let (pdf, docx) = (skill_md("pdf"), skill_md("docx"));
    zip_of(&[
        ("r-HEAD/README.md", b"# repo"),
        ("r-HEAD/skills/pdf/SKILL.md", &pdf),
        ("r-HEAD/skills/pdf/scripts/fill.py", b"print(1)\n"),
        ("r-HEAD/skills/docx/SKILL.md", &docx),
    ])
}

/// A host whose skill downloads go to `server`.
fn start(dir: &Dir, server: &SkillServer) -> Running {
    let origin = server.origin();
    dir.start_with(0, move |config| config.with_skill_server(&origin))
}

fn preview(host: &Running, from: SkillFrom) -> Result<SkillPreview, ErrorCode> {
    match host.query(HostQuery::PreviewSkill { from }) {
        Ok(QueryResult::SkillPreview(preview)) => Ok(preview),
        Ok(other) => panic!("PreviewSkill answered {other:?}"),
        Err(error) => Err(error.code()),
    }
}

fn add(token: &str, pick: &[&str], scope: AddonScope) -> HostCommand {
    HostCommand::AddSkills {
        token: token.to_owned(),
        pick: pick.iter().map(|name| (*name).to_owned()).collect(),
        scope,
    }
}

fn github() -> SkillFrom {
    SkillFrom::Url("https://github.com/o/r".to_owned())
}

#[test]
fn the_search_answers_its_items_and_retries_without_the_last_word() {
    let dir = Dir::new("add_skills_search", &[]);
    let server = SkillServer::start();
    let item = r#"{"id":"o/r/pdf","source":"o/r","skillId":"pdf","name":"pdf","installs":12}"#;
    server.body(
        "/api/search?q=pdf",
        format!(r#"{{"skills":[{item}]}}"#).into_bytes(),
    );
    server.body("/api/search?q=pdf+forms", br#"{"skills":[]}"#.to_vec());
    let host = start(&dir, &server);
    let search = |q: &str| match host.query(HostQuery::SearchSkills { q: q.to_owned() }) {
        Ok(QueryResult::SkillSearch(found)) => found.items,
        other => panic!("SearchSkills answered {other:?}"),
    };
    let hits = search("pdf");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "o/r/pdf");
    assert_eq!(hits[0].installs, 12);
    assert_eq!(hits[0].page, "https://skills.sh/o/r/pdf");
    assert_eq!(
        search("pdf forms").len(),
        1,
        "the retry drops the last word"
    );
    assert_eq!(
        server.hits(),
        [
            "/api/search?q=pdf",
            "/api/search?q=pdf+forms",
            "/api/search?q=pdf"
        ]
    );
    // Only `q` goes: no cookie and no header naming the lawyer, the matter or the app's state.
    for head in server.heads() {
        let names: Vec<String> = head
            .iter()
            .map(|line| {
                line.split(':')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
            })
            .collect();
        assert!(!names.iter().any(|name| name == "cookie"), "{head:?}");
        let known = [
            "host",
            "user-agent",
            "accept",
            "accept-encoding",
            "connection",
        ];
        assert!(
            names.iter().all(|name| known.contains(&name.as_str())),
            "{head:?}"
        );
    }
}

/// A PNG of `len` bytes: the signature, then zeros.
fn png(len: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.resize(len, 0);
    bytes
}

#[test]
fn a_github_repository_previews_and_a_picked_skill_is_listed_with_its_source() {
    let dir = Dir::new("add_skills_github", &[]);
    let server = SkillServer::start();
    server.body("/o/r/zip/HEAD", repo_archive());
    server.body("/o.png", png(100));
    let host = start(&dir, &server);
    let shown = preview(&host, github()).unwrap();
    assert_eq!(shown.from.kind, AddonFromKind::Github);
    assert_eq!(shown.from.text, "o/r");
    let mut skills: Vec<(&str, bool, bool)> = shown
        .skills
        .iter()
        .map(|s| (s.name.as_str(), s.has_scripts, s.already_added))
        .collect();
    skills.sort_unstable();
    assert_eq!(skills, [("docx", false, false), ("pdf", true, false)]);

    host.ok(add(&shown.token, &["pdf"], AddonScope::All));
    let items = addons(&host);
    let pdf = item(&items, AddonKind::Skill, "pdf");
    let from = pdf.from.as_ref().unwrap();
    assert_eq!(
        (from.kind, from.text.as_str()),
        (AddonFromKind::Github, "o/r")
    );
    assert_eq!(pdf.has_scripts, Some(true));
    assert!(pdf.on, "{pdf:?}");
    let icon = pdf.icon.as_deref().unwrap();
    assert!(
        icon.starts_with("data:image/png;base64,iVBORw0KGgo"),
        "{icon}"
    );
    assert!(items.iter().all(|row| row.id != "docx"));
    // The owner's picture was asked once, at the add, with the owner's name alone.
    assert_eq!(
        server.hits().iter().filter(|hit| *hit == "/o.png").count(),
        1
    );

    // The preview is gone once used; a fresh one says pdf was added before.
    host.refused(
        add(&shown.token, &["docx"], AddonScope::All),
        ErrorCode::SkillPreviewExpired,
    );
    let again = preview(&host, github()).unwrap();
    let pdf = again.skills.iter().find(|s| s.name == "pdf").unwrap();
    assert!(pdf.already_added);
}

#[test]
fn an_expired_or_unknown_preview_is_refused() {
    let dir = Dir::new("add_skills_expired", &[]);
    let server = SkillServer::start();
    server.body("/o/r/zip/HEAD", repo_archive());
    let host = start(&dir, &server);
    let shown = preview(&host, github()).unwrap();
    host.clock.advance(Duration::from_mins(31));
    host.refused(
        add(&shown.token, &["pdf"], AddonScope::All),
        ErrorCode::SkillPreviewExpired,
    );
    host.refused(
        add(
            "0123456789abcdef0123456789abcdef",
            &["pdf"],
            AddonScope::All,
        ),
        ErrorCode::SkillPreviewExpired,
    );
}

#[test]
fn a_name_the_preview_lacks_is_command_invalid() {
    let dir = Dir::new("add_skills_invalid", &[]);
    let server = SkillServer::start();
    server.body("/o/r/zip/HEAD", repo_archive());
    let host = start(&dir, &server);
    let shown = preview(&host, github()).unwrap();
    host.refused(
        add(&shown.token, &["xlsx"], AddonScope::All),
        ErrorCode::CommandInvalid,
    );
    assert!(addons(&host).iter().all(|row| row.id != "pdf"));
}

/// While a firm's policy sets `skills.scope`, adding for one matter is `settings.locked` and
/// adds nothing; adding for every matter still works.
#[test]
fn a_firm_lock_on_scopes_refuses_adding_for_a_matter() {
    let dir = Dir::new("add_skills_scope_locked", &[]);
    dir.set_policy(r#"{"skills": {"scope": {"nda-kit": "matter"}}}"#);
    let server = SkillServer::start();
    server.body("/o/r/zip/HEAD", repo_archive());
    let host = start(&dir, &server);
    let (x, _) = two_matters(&dir, &host);
    let shown = preview(&host, github()).unwrap();
    host.refused(
        add(&shown.token, &["pdf"], AddonScope::Matter(x)),
        ErrorCode::SettingsLocked,
    );
    assert!(addons(&host).iter().all(|row| row.id != "pdf"));
    host.ok(add(&shown.token, &["pdf"], AddonScope::All));
    assert!(row(&host, Some(x), AddonKind::Skill, "pdf").on);
    host.shutdown();
}

/// A skill added with scope `matter` X is on in X and off in Y, as `SetAddonScope` leaves it;
/// a matter the host does not know is `command.unknown_matter` and adds nothing.
#[test]
fn a_skill_added_for_a_matter_is_on_there_only() {
    let dir = Dir::new("add_skills_matter", &[]);
    let server = SkillServer::start();
    server.body("/o/r/zip/HEAD", repo_archive());
    let host = start(&dir, &server);
    let (x, y) = two_matters(&dir, &host);
    let shown = preview(&host, github()).unwrap();
    let stranger = MatterId::new(foreign_uuid(1)).unwrap();
    host.refused(
        add(&shown.token, &["pdf"], AddonScope::Matter(stranger)),
        ErrorCode::CommandUnknownMatter,
    );
    assert!(addons(&host).iter().all(|row| row.id != "pdf"));
    host.ok(add(&shown.token, &["pdf"], AddonScope::Matter(x)));
    let in_x = row(&host, Some(x), AddonKind::Skill, "pdf");
    assert!(in_x.on, "{in_x:?}");
    assert_eq!(in_x.scope, AddonScope::Matter(x));
    assert_eq!(in_x.switched, vec![x]);
    let in_y = row(&host, Some(y), AddonKind::Skill, "pdf");
    assert!(!in_y.on, "{in_y:?}");
    host.shutdown();
}

#[test]
fn a_zip_bomb_is_too_big_and_a_404_is_not_found() {
    let dir = Dir::new("add_skills_refused", &[]);
    let server = SkillServer::start();
    let zeros = vec![0_u8; 26 * 1024 * 1024];
    server.body("/bomb.zip", zip_of(&[("zeros.txt", &zeros)]));
    let host = start(&dir, &server);
    let bomb = SkillFrom::Url(format!("{}/bomb.zip", server.origin()));
    // The fake server is plain http: the release client would refuse it before any request.
    assert_eq!(preview(&host, bomb).unwrap_err(), ErrorCode::CommandInvalid);
    let named = SkillFrom::SkillsSh("o/missing".to_owned());
    assert_eq!(preview(&host, named).unwrap_err(), ErrorCode::SkillNotFound);
    server.body("/o/big/zip/HEAD", zip_of(&[("big-HEAD/zeros.txt", &zeros)]));
    let big = SkillFrom::SkillsSh("o/big".to_owned());
    assert_eq!(preview(&host, big).unwrap_err(), ErrorCode::SkillTooBig);
}

#[test]
fn an_icon_over_its_cap_is_left_out() {
    let dir = Dir::new("add_skills_icon_cap", &[]);
    let server = SkillServer::start();
    server.body("/o/r/zip/HEAD", repo_archive());
    server.body("/o.png", png(64 * 1024 + 1));
    let host = start(&dir, &server);
    let shown = preview(&host, github()).unwrap();
    host.ok(add(&shown.token, &["docx"], AddonScope::All));
    let items = addons(&host);
    let docx = item(&items, AddonKind::Skill, "docx");
    assert_eq!(docx.icon, None);
    assert_eq!(docx.has_scripts, Some(false));
    assert!(!dir.data().join("skill-icons/docx.png").exists());
}
