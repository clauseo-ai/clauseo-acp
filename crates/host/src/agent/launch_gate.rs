//! Desktop's launch gate (R17, stage D's #554, `HostConfig::with_launch_gate`): a cheap look
//! at an agent's shared login file just before each launch. This is PR-C's minimal form, to be
//! replaced by #554's when it lands: the type has #554's shape, and the host only requires that
//! one is wired. Every launch of an agent with a kind uses the matter's state folder, whose
//! Codex `auth.json` (and Claude's store) is the one shared login, writable from every matter:
//! with no identity check wired, the launch is refused (`launch_checks.rs`). #554 asks the gate
//! on the same prepare thread.

use std::sync::Arc;

use clauseo_protocol::{AgentId, ErrorCode};

/// Desktop's launch gate: asked with the agent's id just before each launch, it answers whether
/// that launch must not start now (`None`: go on). Cheap and non-blocking: a stat of the login
/// file, never an open of its contents. #554 defines how the host acts on `Some(code)`.
pub type LaunchGate = Arc<dyn Fn(&AgentId) -> Option<ErrorCode> + Send + Sync>;
