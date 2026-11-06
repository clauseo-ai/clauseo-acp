//! Desktop's launch gate (R17, stage D's #554, `HostConfig::with_launch_gate`): before each launch
//! #554's gate stats the shared login file and, when its stamp (size, modified time, file id)
//! changed, re-reads the account from the agent's own status output and refuses the launch when the
//! account is not the one desktop knows. This is PR-C's stand-in, to be replaced by #554's when it
//! lands: the public type [`LaunchGate`] and `HostConfig::with_launch_gate` are identical to
//! #554's, so the rebase is a clean swap, and the host here only requires that one is wired. Every
//! launch of an agent with a kind uses the matter's state folder, whose Codex `auth.json` (and
//! Claude's store) is the one shared login, writable from every matter: with no identity check
//! wired, the launch is refused (`launch_checks.rs`). #554 asks the gate on the same prepare
//! thread.

use std::sync::Arc;

use clauseo_protocol::{AgentId, ErrorCode};

/// Desktop's launch gate: asked with the agent's id just before each launch, it answers whether
/// that launch must not start now (`None`: go on). Identical to #554's type
/// (`Arc<dyn Fn(&AgentId) -> Option<ErrorCode> + Send + Sync>`); #554 defines how the host acts
/// on `Some(code)`.
pub type LaunchGate = Arc<dyn Fn(&AgentId) -> Option<ErrorCode> + Send + Sync>;
