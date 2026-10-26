//! `initialize`, the version check, authentication and `session/load`.

use clauseo_acp::{AgentUpdate, AuthKind, Error, LoadSession, MessageRole, Origin, ToolCall};
use clauseo_protocol::{ConfigOptionKind, TextOrBlob, ToolCallContent, ToolCallStatus, ToolKind};

use crate::harness::{self, Ctx, chunk, inside, sess1, update_event};

#[test]
fn initialize_caps_exact() {
    let outcome = harness::run("initialize-caps", async |ctx: Ctx| {
        let info = ctx.client.initialize().await.unwrap();
        assert_eq!(info.name.as_deref(), Some("fake_agent"));
        assert_eq!(info.version.as_deref(), Some("0.0.0"));
        assert!(info.capabilities.load_session);
        assert!(info.capabilities.prompt.embedded_context);
        assert!(!info.capabilities.prompt.image);
        assert!(info.capabilities.session.list && info.capabilities.session.resume);
        assert_eq!(info.capabilities.steering, None);
        let kinds: Vec<_> = info
            .auth_methods
            .iter()
            .map(|m| (m.id.as_str(), &m.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("fake-login", &AuthKind::Agent),
                (
                    "fake-terminal-login",
                    &AuthKind::Terminal {
                        args: vec!["--login".to_owned()],
                        env: Vec::new()
                    }
                ),
            ]
        );
        let start = ctx.client.new_session(ctx.new_session()).await.unwrap();
        assert_eq!(start.session.as_str(), "sess-1");
        let modes = start.modes.expect("modes");
        assert_eq!(modes.current, "default");
        let ids: Vec<_> = modes.available.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["default", "plan"]);
        assert_eq!(start.config_options.len(), 1);
        let option = &start.config_options[0];
        assert_eq!(option.id, "model");
        assert!(
            matches!(&option.kind, ConfigOptionKind::Select { current_value, options }
            if current_value == "fast" && options.len() == 2)
        );
        assert_eq!(ctx.drain(), Vec::new());
    });
    outcome.assert_clean();
}

#[test]
fn version_mismatch_closes() {
    let outcome = harness::run(&harness::local("version-mismatch"), async |ctx: Ctx| {
        let answer = ctx.client.initialize().await;
        assert_eq!(answer.err(), Some(Error::UnsupportedVersion { agent: 2 }));
    });
    assert_eq!(outcome.driver, Err(Error::UnsupportedVersion { agent: 2 }));
    assert_eq!(outcome.exit, Some(0));
}

#[test]
fn auth_required_then_authenticate() {
    let outcome = harness::run("auth-required", async |ctx: Ctx| {
        let info = ctx.client.initialize().await.unwrap();
        assert_eq!(info.auth_methods.len(), 1);
        let Err(Error::AuthRequired { methods }) = ctx.client.new_session(ctx.new_session()).await
        else {
            panic!("session/new should need authentication");
        };
        let ids: Vec<_> = methods.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["fake-login"]);
        ctx.client.authenticate("fake-login").await.unwrap();
        let start = ctx.client.new_session(ctx.new_session()).await.unwrap();
        assert_eq!(start.session, sess1());
        assert_eq!(ctx.drain(), Vec::new());
    });
    outcome.assert_clean();
}

#[test]
fn load_replay_tagged_replay() {
    let outcome = harness::run("load-replay", async |ctx: Ctx| {
        ctx.client.initialize().await.unwrap();
        let request = LoadSession {
            session: sess1(),
            cwd: ctx.cwd.clone(),
            mcp_servers: Vec::new(),
        };
        let start = ctx.client.load_session(request).await.unwrap();
        assert_eq!(start.session, sess1());
        // The replayed updates arrived before the load response, so they are all here.
        let edit = AgentUpdate::ToolCall(ToolCall {
            acp_id: "tc-edit-0".into(),
            title: "Edit brief.txt".into(),
            kind: ToolKind::Edit,
            tool_name: Some("Edit".into()),
            status: ToolCallStatus::Completed,
            locations: vec![inside("brief.txt")],
            raw_input: None,
            raw_output: None,
            content: vec![ToolCallContent::Diff {
                path: inside("brief.txt"),
                old: Some(TextOrBlob::Inline("Clause 4 draft\n".to_owned())),
                new: TextOrBlob::Inline("Clause 4 final\n".to_owned()),
            }],
        });
        let replay = Origin::Replay;
        assert_eq!(
            ctx.drain(),
            [
                update_event(
                    replay,
                    chunk(MessageRole::User, "m-user-0", "Tighten clause 4")
                ),
                update_event(
                    replay,
                    chunk(
                        MessageRole::Thought,
                        "m-thought-0",
                        "Reading clause 4 first."
                    )
                ),
                update_event(replay, edit),
                update_event(
                    replay,
                    chunk(
                        MessageRole::Answer,
                        "m-answer-0",
                        "I changed clause 4 to the final wording."
                    )
                ),
            ]
        );
    });
    outcome.assert_clean();
}
