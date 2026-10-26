//! Session state: plans, commands, modes, config options, notices, capabilities.

use std::collections::BTreeMap;

use agent_client_protocol::schema::v1 as acp;
use clauseo_protocol::{
    AgentCapabilities, AgentChoiceId, AvailableCommand, CompactString, ConfigCategory,
    ConfigChoice, ConfigOption, ConfigOptionKind, NoticeLevel, PlanEntry, PlanPriority, PlanStatus,
    PromptCapabilities, SessionCapabilities, StopReason,
};

use crate::event::AgentUpdate;
use crate::types::{AgentInfo, AuthKind, AuthMethod, SessionMode, SessionModes};

pub(crate) fn plan(entries: Vec<acp::PlanEntry>) -> Vec<PlanEntry> {
    entries
        .into_iter()
        .map(|entry| PlanEntry {
            content: entry.content,
            priority: match entry.priority {
                acp::PlanEntryPriority::High => PlanPriority::High,
                acp::PlanEntryPriority::Low => PlanPriority::Low,
                // A priority newer than schema 1.11.0 counts as medium.
                acp::PlanEntryPriority::Medium | _ => PlanPriority::Medium,
            },
            status: match entry.status {
                acp::PlanEntryStatus::InProgress => PlanStatus::InProgress,
                acp::PlanEntryStatus::Completed => PlanStatus::Completed,
                // A status newer than schema 1.11.0 counts as pending.
                acp::PlanEntryStatus::Pending | _ => PlanStatus::Pending,
            },
        })
        .collect()
}

pub(crate) fn commands(commands: Vec<acp::AvailableCommand>) -> Vec<AvailableCommand> {
    commands
        .into_iter()
        .map(|command| AvailableCommand {
            name: command.name.into(),
            description: command.description,
            input_hint: command.input.and_then(|input| match input {
                acp::AvailableCommandInput::Unstructured(input) => Some(input.hint),
                // An input kind newer than schema 1.11.0 has no hint to show.
                _ => None,
            }),
        })
        .collect()
}

/// The agent's id as text a command can name back, or why not (`AgentChoiceId`'s rules).
fn choice_id(id: &str, what: &str) -> Result<CompactString, String> {
    AgentChoiceId::new(id)
        .map(|_| CompactString::from(id))
        .map_err(|error| format!("{what} id {id:?}: {error}"))
}

/// Config options. An option or select value whose id no command could name fails them all.
pub(crate) fn config_options(
    options: Vec<acp::SessionConfigOption>,
) -> Result<Vec<ConfigOption>, String> {
    options
        .into_iter()
        .map(|option| {
            let kind = match option.kind {
                acp::SessionConfigKind::Select(select) => ConfigOptionKind::Select {
                    current_value: choice_id(&select.current_value.0, "config value")?,
                    options: choices(select.options)?,
                },
                acp::SessionConfigKind::Boolean(boolean) => ConfigOptionKind::Boolean {
                    current_value: boolean.current_value,
                },
                // A kind newer than schema 1.11.0: no command could set it, so it is left out.
                _ => return Ok(None),
            };
            Ok(Some(ConfigOption {
                id: choice_id(&option.id.0, "config option")?,
                name: option.name.into(),
                description: option.description,
                category: match option.category {
                    Some(acp::SessionConfigOptionCategory::Mode) => ConfigCategory::Mode,
                    Some(acp::SessionConfigOptionCategory::Model) => ConfigCategory::Model,
                    Some(acp::SessionConfigOptionCategory::ThoughtLevel) => {
                        ConfigCategory::ThoughtLevel
                    }
                    Some(
                        acp::SessionConfigOptionCategory::ModelConfig
                        | acp::SessionConfigOptionCategory::Other(_)
                        | _,
                    )
                    | None => ConfigCategory::Other,
                },
                kind,
            }))
        })
        .filter_map(Result::transpose)
        .collect()
}

fn choices(options: acp::SessionConfigSelectOptions) -> Result<Vec<ConfigChoice>, String> {
    let choice = |option: acp::SessionConfigSelectOption, group: Option<String>| {
        Ok(ConfigChoice {
            value: choice_id(&option.value.0, "config value")?,
            name: option.name.into(),
            description: option.description,
            group,
        })
    };
    match options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => options
            .into_iter()
            .map(|option| choice(option, None))
            .collect(),
        acp::SessionConfigSelectOptions::Grouped(groups) => {
            let mut all = Vec::new();
            for group in groups {
                for option in group.options {
                    all.push(choice(option, Some(group.name.clone()))?);
                }
            }
            Ok(all)
        }
        _ => Err("a select option list of a shape newer than schema 1.11.0".to_owned()),
    }
}

/// The modes. A mode whose id no command could name fails them all.
pub(crate) fn modes(state: acp::SessionModeState) -> Result<SessionModes, String> {
    Ok(SessionModes {
        current: choice_id(&state.current_mode_id.0, "mode")?,
        available: state
            .available_modes
            .into_iter()
            .map(|mode| {
                Ok(SessionMode {
                    id: choice_id(&mode.id.0, "mode")?,
                    name: mode.name,
                    description: mode.description,
                })
            })
            .collect::<Result<_, String>>()?,
    })
}

pub(crate) fn notice(notice: acp::Notice) -> AgentUpdate {
    let level = match notice.severity {
        acp::NoticeSeverity::Warning | acp::NoticeSeverity::Error => NoticeLevel::Warning,
        acp::NoticeSeverity::Info | acp::NoticeSeverity::Other(_) | _ => NoticeLevel::Info,
    };
    let text = match notice.description {
        Some(description) => format!("{}\n\n{description}", notice.title),
        None => notice.title,
    };
    AgentUpdate::Notice { level, text }
}

pub(crate) fn stop_reason(reason: acp::StopReason) -> StopReason {
    match reason {
        acp::StopReason::EndTurn => StopReason::EndTurn,
        acp::StopReason::MaxTokens => StopReason::MaxTokens,
        acp::StopReason::MaxTurnRequests => StopReason::MaxTurnRequests,
        acp::StopReason::Refusal => StopReason::Refusal,
        acp::StopReason::Cancelled => StopReason::Cancelled,
        // A reason newer than schema 1.11.0: the turn ended, but not as Clauseo knows.
        _ => StopReason::Failed,
    }
}

/// What `initialize` told. `steering` stays `None` until steer is built.
pub(crate) fn agent_info(response: acp::InitializeResponse) -> AgentInfo {
    let capabilities = response.agent_capabilities;
    let prompt = capabilities.prompt_capabilities;
    let session = capabilities.session_capabilities;
    let (name, version) = response
        .agent_info
        .map_or((None, None), |info| (Some(info.name), Some(info.version)));
    AgentInfo {
        name,
        version,
        capabilities: AgentCapabilities {
            load_session: capabilities.load_session,
            prompt: PromptCapabilities {
                image: prompt.image,
                audio: prompt.audio,
                embedded_context: prompt.embedded_context,
            },
            session: SessionCapabilities {
                list: session.list.is_some(),
                resume: session.resume.is_some(),
                close: session.close.is_some(),
            },
            steering: None,
        },
        auth_methods: response
            .auth_methods
            .into_iter()
            .filter_map(auth_method)
            .collect(),
    }
}

/// A method of a kind newer than schema 1.11.0 is left out: the client cannot run it.
fn auth_method(method: acp::AuthMethod) -> Option<AuthMethod> {
    Some(match method {
        acp::AuthMethod::Agent(agent) => AuthMethod {
            id: CompactString::from(&*agent.id.0),
            name: agent.name,
            description: agent.description,
            kind: AuthKind::Agent,
        },
        acp::AuthMethod::Terminal(terminal) => AuthMethod {
            id: CompactString::from(&*terminal.id.0),
            name: terminal.name,
            description: terminal.description,
            kind: AuthKind::Terminal {
                args: terminal.args,
                env: terminal
                    .env
                    .into_iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            },
        },
        _ => return None,
    })
}
