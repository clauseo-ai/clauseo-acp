//! [`Driver`] and its command loop: the one future the host spawns. It runs the SDK connection
//! over the line filter, and turns each [`Command`] into a request whose response callback runs
//! in the SDK's dispatch order, so an update after a response is tagged as after it.

use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::{Context, Poll};

use agent_client_protocol::schema::v1 as acp;
use agent_client_protocol::{Agent, ConnectionTo, JsonRpcRequest};
use clauseo_protocol::{AgentChoiceId, AgentSessionId, CompactString, ConfigValue, TurnId};
use futures::future::Either;

mod answers;

use crate::Error;
use crate::client::{Command, Reply};
use crate::event::{AcpEvent, Origin};
use crate::map;
use crate::state::{SessionState, Shared};
use crate::types::{AuthKind, McpServer, PromptEnd, SessionStart};

/// The connection's work: reading the agent, dispatching, sending. The host spawns it on its
/// own executor. It ends with `Ok(())` once every [`AcpClient`](crate::AcpClient) is dropped
/// (it then closes the agent's stdin), or with an [`Error`]: `Closed` when the agent's stdout
/// ends first, `Protocol` for a line over 8 MiB, `UnsupportedVersion` after `initialize`.
#[must_use = "the host must spawn the driver, or nothing reaches the agent"]
pub struct Driver(pub(crate) Pin<Box<dyn Future<Output = Result<(), Error>> + Send>>);

impl Future for Driver {
    type Output = Result<(), Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().0.as_mut().poll(cx)
    }
}

impl std::fmt::Debug for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Driver")
    }
}

/// The command loop, the SDK connection's main function. Returns `Ok` when every client is
/// dropped, and an error when [`Shared::stop`] asks.
pub(crate) async fn run(
    cx: ConnectionTo<Agent>,
    commands: async_channel::Receiver<Command>,
    stop: async_channel::Receiver<()>,
    shared: Arc<Shared>,
) -> Result<(), agent_client_protocol::Error> {
    loop {
        let command = pin!(commands.recv());
        let stopped = pin!(stop.recv());
        let next = futures::future::select(command, stopped).await;
        match next {
            Either::Left((Ok(command), _)) => execute(&cx, &shared, command),
            Either::Left((Err(async_channel::RecvError), _)) => return Ok(()),
            Either::Right(_) => {
                return Err(
                    agent_client_protocol::Error::internal_error().data("connection closed")
                );
            }
        }
    }
}

fn execute(cx: &ConnectionTo<Agent>, shared: &Arc<Shared>, command: Command) {
    match command {
        Command::Initialize(reply) => initialize(cx, shared, reply),
        Command::Authenticate { method_id, reply } => {
            let terminal = shared.state().auth_methods.iter().any(|method| {
                method.id == method_id && matches!(method.kind, AuthKind::Terminal { .. })
            });
            if terminal {
                return answer(reply, Err(Error::TerminalAuthMethod));
            }
            let request = acp::AuthenticateRequest::new(method_id);
            send(cx, shared, request, reply, |_| async { Ok(()) });
        }
        Command::NewSession { request, reply } => new_session(cx, shared, request, reply),
        Command::LoadSession { request, reply } => load_session(cx, shared, request, reply),
        Command::Prompt {
            session,
            turn,
            content,
            reply,
        } => prompt(cx, shared, &session, turn, content, reply),
        Command::PromptDropped { session, turn } => {
            let mut state = shared.state();
            if let Some(entry) = state.sessions.get_mut(session.as_str())
                && entry.prompt == Some(turn)
                && entry.origin == Origin::Late(turn)
            {
                entry.prompt = None;
            }
        }
        Command::Cancel { session, reply } => answer(reply, answers::cancel(cx, shared, &session)),
        Command::AnswerPermission {
            key,
            option_id,
            reply,
        } => answer(reply, answers::permission(shared, key, &option_id)),
        Command::AnswerElicitation {
            key,
            response,
            reply,
        } => answer(reply, answers::elicitation(shared, key, response)),
        Command::SetConfigOption {
            session,
            option_id,
            value,
            reply,
        } => set_config_option(cx, shared, &session, &option_id, value, reply),
        Command::SetMode {
            session,
            mode_id,
            reply,
        } => {
            if !known(shared, &session) {
                return answer(reply, Err(Error::UnknownSession));
            }
            let request = acp::SetSessionModeRequest::new(
                session.as_str().to_owned(),
                mode_id.as_str().to_owned(),
            );
            send(cx, shared, request, reply, |_| async { Ok(()) });
        }
    }
}

fn new_session(
    cx: &ConnectionTo<Agent>,
    shared: &Arc<Shared>,
    request: crate::NewSession,
    reply: Reply<SessionStart>,
) {
    let cwd = request.cwd;
    let servers = request
        .mcp_servers
        .into_iter()
        .map(mcp_server)
        .collect::<Vec<_>>();
    let mut sent = acp::NewSessionRequest::new(cwd.clone()).mcp_servers(servers);
    sent.meta = request.meta;
    let state = Arc::clone(shared);
    send(cx, shared, sent, reply, move |response| async move {
        let start = session_start(
            &response.session_id.0,
            response.modes,
            response.config_options,
        )?;
        let session = CompactString::from(start.session.as_str());
        let entry = SessionState::new(cwd, Origin::Idle);
        state.state().sessions.insert(session, entry);
        Ok(start)
    });
}

fn set_config_option(
    cx: &ConnectionTo<Agent>,
    shared: &Arc<Shared>,
    session: &AgentSessionId,
    option_id: &AgentChoiceId,
    value: ConfigValue,
    reply: Reply<Vec<clauseo_protocol::ConfigOption>>,
) {
    if !known(shared, session) {
        return answer(reply, Err(Error::UnknownSession));
    }
    let value = match value {
        ConfigValue::Select(choice) => {
            acp::SessionConfigOptionValue::value_id(choice.as_str().to_owned())
        }
        ConfigValue::Boolean(value) => acp::SessionConfigOptionValue::boolean(value),
    };
    let request = acp::SetSessionConfigOptionRequest::new(
        session.as_str().to_owned(),
        option_id.as_str().to_owned(),
        value,
    );
    send(cx, shared, request, reply, |response| async move {
        map::config_options(response.config_options).map_err(Error::Protocol)
    });
}

fn initialize(cx: &ConnectionTo<Agent>, shared: &Arc<Shared>, reply: Reply<crate::AgentInfo>) {
    let request = acp::InitializeRequest::new(agent_client_protocol::schema::ProtocolVersion::V1)
        .client_capabilities(client_capabilities())
        .client_info(acp::Implementation::new(
            "clauseo",
            env!("CARGO_PKG_VERSION"),
        ));
    let state = Arc::clone(shared);
    send(cx, shared, request, reply, move |response| async move {
        let version = response.protocol_version.as_u16();
        if version != 1 {
            let error = Error::UnsupportedVersion { agent: version };
            state.stop(error.clone());
            return Err(error);
        }
        let info = map::agent_info(response);
        let mut locked = state.state();
        locked.prompt_capabilities = info.capabilities.prompt;
        locked.auth_methods.clone_from(&info.auth_methods);
        Ok(info)
    });
}

/// What Clauseo advertises: no `fs`, no `terminal`, no `subagents`; terminal sign-in, form and
/// URL elicitation, notices, compaction, boolean config options, `plan`, and the AIR
/// `sessionFailure` capability in `clientCapabilities._meta`.
pub(crate) fn client_capabilities() -> acp::ClientCapabilities {
    let mut air = serde_json::Map::new();
    air.insert(
        "jetbrains".to_owned(),
        serde_json::json!({"air": {"version": 1, "capabilities": ["sessionFailure"]}}),
    );
    acp::ClientCapabilities::new()
        .auth(acp::AuthCapabilities::new().terminal(true))
        .elicitation(
            acp::ElicitationCapabilities::new()
                .form(acp::ElicitationFormCapabilities::new())
                .url(acp::ElicitationUrlCapabilities::new()),
        )
        .session(
            acp::ClientSessionCapabilities::new()
                .notices(acp::NoticeCapabilities::new())
                .compaction(acp::CompactionCapabilities::new())
                .config_options(
                    acp::SessionConfigOptionsCapabilities::new()
                        .boolean(acp::BooleanConfigOptionCapabilities::new()),
                ),
        )
        .plan(acp::PlanCapabilities::new())
        .meta(air)
}

fn load_session(
    cx: &ConnectionTo<Agent>,
    shared: &Arc<Shared>,
    request: crate::LoadSession,
    reply: Reply<SessionStart>,
) {
    let key = CompactString::from(request.session.as_str());
    let entry = SessionState::new(request.cwd.clone(), Origin::Replay);
    shared.state().sessions.insert(key.clone(), entry);
    let sent = acp::LoadSessionRequest::new(request.session.as_str().to_owned(), request.cwd)
        .mcp_servers(
            request
                .mcp_servers
                .into_iter()
                .map(mcp_server)
                .collect::<Vec<_>>(),
        );
    let state = Arc::clone(shared);
    let session = request.session;
    let on_answer = move |result: Result<acp::LoadSessionResponse, Error>| async move {
        let start = result.and_then(|response| {
            session_start(session.as_str(), response.modes, response.config_options)
        });
        let mut locked = state.state();
        if start.is_ok() {
            if let Some(entry) = locked.sessions.get_mut(&key) {
                entry.origin = Origin::Idle;
            }
        } else {
            locked.sessions.remove(&key);
        }
        start
    };
    send_with(cx, shared, sent, reply, on_answer);
}

fn prompt(
    cx: &ConnectionTo<Agent>,
    shared: &Arc<Shared>,
    session: &AgentSessionId,
    turn: TurnId,
    content: Vec<clauseo_protocol::ContentBlock>,
    reply: Reply<PromptEnd>,
) {
    let blocks = {
        let mut state = shared.state();
        let capabilities = state.prompt_capabilities;
        let Some(entry) = state.sessions.get_mut(session.as_str()) else {
            return answer(reply, Err(Error::UnknownSession));
        };
        if entry.prompt.is_some() {
            return answer(reply, Err(Error::TurnRunning));
        }
        let blocks = content
            .into_iter()
            .map(|block| map::prompt_block(block, capabilities))
            .collect::<Result<Vec<_>, Error>>();
        let blocks = match blocks {
            Ok(blocks) => blocks,
            Err(error) => return answer(reply, Err(error)),
        };
        entry.prompt = Some(turn);
        entry.origin = Origin::Turn(turn);
        blocks
    };
    let request = acp::PromptRequest::new(session.as_str().to_owned(), blocks);
    let state = Arc::clone(shared);
    let key = CompactString::from(session.as_str());
    let on_answer = move |result: Result<acp::PromptResponse, Error>| async move {
        {
            let mut locked = state.state();
            if let Some(entry) = locked.sessions.get_mut(&key)
                && entry.prompt == Some(turn)
            {
                entry.prompt = None;
                if entry.origin == Origin::Turn(turn) {
                    entry.origin = Origin::Idle;
                }
            }
        }
        let response = result?;
        let mut limit = None;
        if let Some(failure) = response.meta.as_ref().and_then(map::session_failure) {
            match failure {
                Ok(signal) => limit = Some(signal),
                Err(warning) => state.emit(AcpEvent::Warning(warning)).await,
            }
        }
        Ok(PromptEnd {
            stop: map::stop_reason(response.stop_reason),
            limit,
        })
    };
    send_with(cx, shared, request, reply, on_answer);
}

pub(crate) fn known(shared: &Shared, session: &AgentSessionId) -> bool {
    shared.state().sessions.contains_key(session.as_str())
}

fn session_start(
    session: &str,
    modes: Option<acp::SessionModeState>,
    config_options: Option<Vec<acp::SessionConfigOption>>,
) -> Result<SessionStart, Error> {
    let session = AgentSessionId::new(session)
        .map_err(|error| Error::Protocol(format!("session id: {error}")))?;
    Ok(SessionStart {
        session,
        modes: modes.map(map::modes).transpose().map_err(Error::Protocol)?,
        config_options: map::config_options(config_options.unwrap_or_default())
            .map_err(Error::Protocol)?,
    })
}

fn mcp_server(server: McpServer) -> acp::McpServer {
    match server {
        McpServer::Stdio {
            name,
            command,
            args,
            env,
        } => acp::McpServer::Stdio(
            acp::McpServerStdio::new(name, command).args(args).env(
                env.into_iter()
                    .map(|(name, value)| acp::EnvVariable::new(name, value))
                    .collect::<Vec<_>>(),
            ),
        ),
    }
}

/// Sends `request`; `on_ok` maps a successful response, in the SDK's dispatch order.
fn send<Req, T, F>(
    cx: &ConnectionTo<Agent>,
    shared: &Arc<Shared>,
    request: Req,
    reply: Reply<T>,
    on_ok: impl FnOnce(Req::Response) -> F + Send + 'static,
) where
    Req: JsonRpcRequest,
    T: Send + 'static,
    F: Future<Output = Result<T, Error>> + Send + 'static,
{
    send_with(cx, shared, request, reply, move |result| async move {
        match result {
            Ok(response) => on_ok(response).await,
            Err(error) => Err(error),
        }
    });
}

/// Sends `request`; `on_answer` sees the response or the error and runs in the SDK's dispatch
/// order, before any later message from the agent. Its answer goes to `reply`, or nowhere when
/// the call's future was dropped.
fn send_with<Req, T, F>(
    cx: &ConnectionTo<Agent>,
    shared: &Arc<Shared>,
    request: Req,
    reply: Reply<T>,
    on_answer: impl FnOnce(Result<Req::Response, Error>) -> F + Send + 'static,
) where
    Req: JsonRpcRequest,
    T: Send + 'static,
    F: Future<Output = Result<T, Error>> + Send + 'static,
{
    let shared = Arc::clone(shared);
    // An error means the connection is closed: the callback and `reply` are dropped, and the
    // caller sees `Error::Closed`.
    let sent = cx
        .prepare_request(request)
        .on_receiving_result(move |result| async move {
            let result = result.map_err(|error| rpc_error(&shared, error));
            answer(reply, on_answer(result).await);
            Ok(())
        });
    match sent {
        Ok(()) | Err(_) => {}
    }
}

/// An error the agent answered with, or the SDK's for a connection that closed first.
fn rpc_error(shared: &Shared, error: agent_client_protocol::Error) -> Error {
    if agent_client_protocol::is_incoming_transport_closed(&error) {
        return Error::Closed;
    }
    if error.code == acp::ErrorCode::AuthRequired {
        let methods = shared.state().auth_methods.clone();
        return Error::AuthRequired { methods };
    }
    Error::Rpc {
        code: error.code.into(),
        message: error.message,
    }
}

pub(crate) fn answer<T>(reply: Reply<T>, result: Result<T, Error>) {
    // An error means the call's future was dropped; its answer is discarded.
    match reply.send(result) {
        Ok(()) | Err(_) => {}
    }
}
