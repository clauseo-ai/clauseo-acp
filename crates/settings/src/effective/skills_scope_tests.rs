//! A skill pack's scope (`skills.scope`) and an MCP server's (`mcp.servers.<name>.scope`): the
//! typing of the key, the writes, a fresh load that agrees, and the firm's lock.

use std::collections::BTreeMap;

use super::Effective;
use crate::error::Error;
use crate::schema::{ItemScope, McpServerLaunch};
use crate::testing::{MemFiles, abs_str, locations, policy, user};
use crate::warning::WarningKind;

#[test]
fn skills_scope_types_entry_by_entry() {
    let files = user(
        r#"{"skills": {"scope": {"pdf": "matter", "tax": "all", "Bad Id": "matter",
        "deeds": "everywhere", "clauseo": "matter"}}}"#,
    );
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.skill_scope("pdf"), ItemScope::Matter);
    assert_eq!(effective.skill_scope("tax"), ItemScope::All);
    assert_eq!(effective.skill_scope("deeds"), ItemScope::All);
    assert_eq!(effective.skill_scope("never"), ItemScope::All);
    let dropped: Vec<&str> = effective
        .warnings()
        .iter()
        .filter_map(|warning| match &warning.kind {
            WarningKind::InvalidValue { key, .. } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(dropped.len(), 3, "{dropped:?}");
    assert!(dropped.contains(&"skills.scope.deeds"), "{dropped:?}");
    assert!(dropped.contains(&"skills.scope.clauseo"), "{dropped:?}");
}

#[test]
fn a_skills_scope_write_keeps_the_rest_and_a_fresh_load_agrees() {
    let files = user(r#"{"ui": {"theme": "dark"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .set_skill_scope(&files, "pdf", ItemScope::Matter)
        .unwrap();
    let again = Effective::load(&locations(), &files, &[]);
    assert_eq!(again.skill_scope("pdf"), ItemScope::Matter);
    assert_eq!(again.settings().ui, effective.settings().ui);
    effective
        .set_skill_scope(&files, "pdf", ItemScope::All)
        .unwrap();
    let again = Effective::load(&locations(), &files, &[]);
    assert_eq!(again.skill_scope("pdf"), ItemScope::All);
    assert!(again.settings().skills.scope.is_empty());
    assert!(matches!(
        effective.set_skill_scope(&files, "clauseo", ItemScope::Matter),
        Err(Error::Refused { key, .. }) if key == "skills.scope"
    ));
}

#[test]
fn a_policy_skills_scope_locks_the_write() {
    let firm = policy("firm", r#"{"skills": {"scope": {"pdf": "matter"}}}"#);
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    assert!(effective.is_locked("skills.scope"));
    assert_eq!(effective.skill_scope("pdf"), ItemScope::Matter);
    assert!(matches!(
        effective.set_skill_scope(&files, "pdf", ItemScope::All),
        Err(Error::Locked { key }) if key == "skills.scope"
    ));
    assert_eq!(files.text(locations().settings_file()), None);
}

#[test]
fn a_servers_scope_is_written_with_its_switch_and_read_back() {
    let files = user(r#"{"ui": {"theme": "dark"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    let server = McpServerLaunch::local(abs_str("bin/notes"), Vec::new(), BTreeMap::new(), false);
    effective
        .save_mcp_server(&files, None, "notes", &server)
        .unwrap();
    effective
        .set_mcp_server_scope(&files, "notes", true, Some(ItemScope::Matter))
        .unwrap();
    let again = Effective::load(&locations(), &files, &[]);
    let held = &again.mcp_servers()["notes"];
    assert!(held.enabled && held.matter_scoped());
    assert!(again.warnings().is_empty(), "{:?}", again.warnings());
    effective
        .set_mcp_server_scope(&files, "notes", true, None)
        .unwrap();
    let again = Effective::load(&locations(), &files, &[]);
    assert_eq!(again.mcp_servers()["notes"].scope, None);
    assert!(matches!(
        effective.set_mcp_server_scope(&files, "nobody", true, None),
        Err(Error::Mcp { .. })
    ));
}
