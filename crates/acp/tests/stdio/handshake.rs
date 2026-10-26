//! `initialize`, the version check, authentication and `session/load`.

use clauseo_acp::{AuthKind, Origin};
use clauseo_protocol::{ConfigOptionKind, ToolLocation};

use crate::harness::{self, Ctx};

#[test]
fn initialize_caps_exact() {
    let outcome = harness::run("initialize-caps", async |ctx: Ctx| {
        let info = ctx.client.initialize().await.unwrap();
        assert_eq!(info.name, "fake_agent");
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
        assert!(ctx.drain().is_empty());
    });
    outcome.assert_clean();
}
