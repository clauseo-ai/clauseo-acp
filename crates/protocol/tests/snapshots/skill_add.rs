//! The wire forms of adding a skill (`skill_add.rs`): `SearchSkills`, `PreviewSkill` with each
//! `from`, their answers, and `AddSkills`' shape rules. Checked as JSON text, so no golden file
//! is needed beyond `host_command`'s `add_skills`.

use super::*;
use clauseo_protocol::{
    AddonFrom, AddonFromKind, AddonScope, CatalogueItem, CommandEnvelope, HostQuery,
    PreviewedSkill, QueryResult, SkillFrom, SkillPreview, SkillSearch,
};
use serde_json::json;

fn text<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn the_queries_read_as_the_contract_writes_them() {
    let search = HostQuery::SearchSkills {
        q: "contract review".to_owned(),
    };
    assert_eq!(
        text(&search),
        json!({"search_skills": {"q": "contract review"}})
    );
    round_trips(&search);
    for (from, body) in [
        (
            SkillFrom::SkillsSh("anthropics/skills".to_owned()),
            json!({"skills_sh": "anthropics/skills"}),
        ),
        (
            SkillFrom::Url("https://github.com/o/r".to_owned()),
            json!({"url": "https://github.com/o/r"}),
        ),
        (
            SkillFrom::Path("/picked/kit.zip".to_owned()),
            json!({"path": "/picked/kit.zip"}),
        ),
    ] {
        let query = HostQuery::PreviewSkill { from };
        assert_eq!(text(&query), json!({"preview_skill": {"from": body}}));
        round_trips(&query);
    }
    let unknown = json!({"preview_skill": {"from": {"git": "x"}}});
    assert!(serde_json::from_value::<HostQuery>(unknown).is_err());
}

#[test]
fn the_answers_read_as_the_contract_writes_them() {
    let preview = QueryResult::SkillPreview(SkillPreview {
        token: "t".to_owned(),
        from: AddonFrom {
            kind: AddonFromKind::Github,
            text: "anthropics/skills".to_owned(),
        },
        skills: vec![PreviewedSkill {
            name: "pdf".to_owned(),
            description: "Reads PDFs.".to_owned(),
            has_scripts: true,
            already_added: false,
        }],
    });
    assert_eq!(
        text(&preview),
        json!({"skill_preview": {"token": "t",
            "from": {"kind": "github", "text": "anthropics/skills"},
            "skills": [{"name": "pdf", "description": "Reads PDFs.", "has_scripts": true,
                "already_added": false}]}})
    );
    round_trips(&preview);
    let search = QueryResult::SkillSearch(SkillSearch {
        items: vec![CatalogueItem {
            id: "anthropics/skills/pdf".to_owned(),
            name: "pdf".to_owned(),
            source: "anthropics/skills".to_owned(),
            installs: 7,
            page: "https://skills.sh/anthropics/skills/pdf".to_owned(),
        }],
    });
    assert_eq!(
        text(&search),
        json!({"skill_search": {"items": [{"id": "anthropics/skills/pdf", "name": "pdf",
            "source": "anthropics/skills", "installs": 7,
            "page": "https://skills.sh/anthropics/skills/pdf"}]}})
    );
    round_trips(&search);
}

#[test]
fn add_skills_takes_a_matter_scope_and_refuses_an_empty_pick() {
    let command: HostCommand = serde_json::from_value(json!({"add_skills": {"token": "t",
        "pick": ["pdf"], "scope": {"matter": matter_id(1)}}}))
    .unwrap();
    assert!(matches!(
        &command,
        HostCommand::AddSkills {
            scope: AddonScope::Matter(_),
            ..
        }
    ));
    let envelope = |pick: Vec<String>, token: &str| CommandEnvelope {
        command_id: fixtures::command_id(9),
        command: HostCommand::AddSkills {
            token: token.to_owned(),
            pick,
            scope: AddonScope::All,
        },
    };
    assert!(envelope(vec!["pdf".to_owned()], "t").validate().is_ok());
    assert!(envelope(Vec::new(), "t").validate().is_err());
    assert!(envelope(vec![String::new()], "t").validate().is_err());
    assert!(envelope(vec!["pdf".to_owned()], "").validate().is_err());
    assert!(envelope(vec!["x".to_owned(); 65], "t").validate().is_err());
}

#[test]
fn an_added_rows_icon_is_optional_on_the_wire() {
    let row = json!({"kind": "skill", "id": "pdf", "name": "pdf", "description": "",
        "source": "added", "scope": "all", "on": true, "status": "ready",
        "can_remove": true, "locked": false, "switched": []});
    let plain: clauseo_protocol::Addon = serde_json::from_value(row.clone()).unwrap();
    assert_eq!(plain.icon, None);
    assert!(text(&plain).get("icon").is_none());
    let mut with_icon = row;
    with_icon["icon"] = json!("data:image/png;base64,iVBORw0KGgo=");
    let read: clauseo_protocol::Addon = serde_json::from_value(with_icon.clone()).unwrap();
    assert_eq!(
        read.icon.as_deref(),
        Some("data:image/png;base64,iVBORw0KGgo=")
    );
    assert_eq!(text(&read), with_icon);
}
