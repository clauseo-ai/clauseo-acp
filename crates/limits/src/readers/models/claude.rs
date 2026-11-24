//! Claude's model probe: the adapter, `initialize`, then `session/options` (module doc above).
//! `session/options` is not in the ACP schema the host's client speaks, so this talks plain
//! JSON-RPC lines.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launcher, Timer};
use clauseo_protocol::{AgentModels, NamedChoice};
use clauseo_settings::AgentLaunch;
use futures::io::BufReader;
use futures::{AsyncBufReadExt as _, AsyncWrite, AsyncWriteExt as _};
use serde_json::{Value, json};

use super::super::claude::CLAUDE_FORCED_VARS;
use super::super::env::spec_env;
use super::super::paths;
use super::super::run::{LINE_MAX, Ran, Stdout, run};
use super::{entry, models_of, named};
use crate::write;

/// How long the probe may take before its tree is killed: Claude Code's start-up, as the limits
/// read allows it.
pub(crate) const LIMIT: Duration = Duration::from_secs(25);
/// The variable naming Claude Code's config folder.
const CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
/// The effort option's id when the answer has none (claude-agent-acp 0.110.1,
/// `dist/session-config-ids.js`).
const EFFORT_ID: &str = "effort";
/// The ids of the two requests.
const INITIALIZE: u64 = 1;
const OPTIONS: u64 = 2;

/// One probe of Claude's models; `None` on any failure.
pub(crate) async fn probe<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    launch: &AgentLaunch,
    run_dir: &Path,
    data_dir: &Path,
    claude_home: Option<&Path>,
) -> Option<AgentModels> {
    let spec = match prepare(launch, run_dir, claude_home, std::env::vars_os()) {
        Ok(spec) => spec,
        Err(message) => {
            tracing::info!(%message, "Claude's model probe did not launch");
            return None;
        }
    };
    let cwd = data_dir.to_string_lossy().into_owned();
    let ran = run(
        launcher,
        &spec,
        timer,
        LIMIT,
        |stdin, stdout| talk(stdin, stdout, cwd),
        Option::is_some,
    )
    .await;
    match ran {
        Ran::Exited {
            talked: Some(models),
            ..
        }
        | Ran::Lingered {
            talked: Some(models),
        } => Some(models),
        Ran::LaunchFailed(_) => {
            tracing::info!("Claude's model probe did not launch");
            None
        }
        Ran::TimedOut => {
            tracing::info!("Claude's model probe timed out");
            None
        }
        Ran::Exited { .. } | Ran::Lingered { .. } => {
            tracing::info!("Claude's model probe got no options");
            None
        }
    }
}

/// The adapter's spec: the entry's command and arguments in `run_dir`, with the inherited
/// environment under the entry's variables, `CLAUDE_CONFIG_DIR` forced to `claude_home` when
/// given, and the variables every Claude launch forces.
pub(crate) fn prepare(
    launch: &AgentLaunch,
    run_dir: &Path,
    claude_home: Option<&Path>,
    inherited: impl Iterator<Item = (OsString, OsString)>,
) -> Result<LaunchSpec, String> {
    let located = paths::locate(paths::CLAUDE_ID, launch).map_err(|err| err.to_string())?;
    paths::check_run_dir(run_dir).map_err(|err| err.to_string())?;
    write::create_dir_all(run_dir).map_err(|err| err.to_string())?;
    paths::check_run_dir(run_dir).map_err(|err| err.to_string())?;
    let forced =
        claude_home.map(|home| (std::ffi::OsStr::new(CLAUDE_CONFIG_DIR), home.as_os_str()));
    let mut env = spec_env(&launch.env, inherited, forced, &[]);
    for (name, value) in CLAUDE_FORCED_VARS {
        env.retain(|key, _| !key.eq_ignore_ascii_case(name));
        env.insert(OsString::from(name), OsString::from(value));
    }
    let args = launch.args.iter().map(OsString::from).collect();
    LaunchSpec::new(located.program, args, env, run_dir.to_owned()).map_err(|err| err.to_string())
}

/// `initialize`, then `session/options`; the models of its answer. Stdin is closed once it
/// came, so the adapter exits.
async fn talk<W>(mut stdin: W, stdout: Stdout, cwd: String) -> (Option<AgentModels>, Stdout)
where
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(stdout);
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": INITIALIZE,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {
                "_meta": {"jetbrains": {"air": {
                    "version": 1,
                    "capabilities": ["sessionFailure", "recommendedValue"],
                }}},
            },
            "clientInfo": {"name": "clauseo", "version": env!("CARGO_PKG_VERSION")},
        },
    });
    let options = json!({
        "jsonrpc": "2.0",
        "id": OPTIONS,
        "method": "session/options",
        "params": {"cwd": cwd, "mcpServers": []},
    });
    let mut answer = None;
    if send(&mut stdin, &initialize).await
        && response(&mut reader, INITIALIZE).await.is_some()
        && send(&mut stdin, &options).await
    {
        answer = response(&mut reader, OPTIONS)
            .await
            .and_then(|result| models(&result));
    }
    let _closed = stdin.close().await;
    (answer, Box::new(reader))
}

async fn send<W: AsyncWrite + Unpin>(stdin: &mut W, message: &Value) -> bool {
    let mut line = message.to_string().into_bytes();
    line.push(b'\n');
    stdin.write_all(&line).await.is_ok() && stdin.flush().await.is_ok()
}

/// The `result` of the response to request `id`, skipping every other message; `None` for an
/// error response, a line over [`LINE_MAX`] or the end of stdout.
async fn response(reader: &mut BufReader<Stdout>, id: u64) -> Option<Value> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).await.ok()?;
        if read == 0 || line.len() > LINE_MAX {
            return None;
        }
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if message.get("method").is_some() || message.get("id").and_then(Value::as_u64) != Some(id)
        {
            continue;
        }
        return message.get("result").cloned();
    }
}

/// The models of a `session/options` answer: its `model` select (the per-model efforts from each
/// choice's AIR metadata) and its `thought_level` select (the current model's efforts and the
/// current effort).
pub(crate) fn models(result: &Value) -> Option<AgentModels> {
    let options = result.get("configOptions")?.as_array()?;
    let of = |category: &str| {
        options
            .iter()
            .find(|option| option.get("category").and_then(Value::as_str) == Some(category))
    };
    let model = of("model")?;
    let effort = of("thought_level");
    let model_id = model.get("id")?.as_str()?;
    let current = model.get("currentValue").and_then(Value::as_str);
    let effort_choices = effort.map(choices).unwrap_or_default();
    let current_effort = effort.and_then(|option| option.get("currentValue")?.as_str());
    let entries = choices(model)
        .iter()
        .filter_map(|choice| {
            let id = choice.get("value")?.as_str()?;
            let name = choice.get("name").and_then(Value::as_str).unwrap_or(id);
            let description = choice.get("description").and_then(Value::as_str);
            let air = choice.pointer("/_meta/jetbrains/air/reasoningEfforts");
            let (efforts, recommended) = match air {
                Some(air) => (air_efforts(air, &effort_choices), air.get("recommended")),
                // The current model without AIR metadata: the effort option's own levels.
                None if Some(id) == current => (named_choices(&effort_choices), None),
                None => (Vec::new(), None),
            };
            let default = recommended.and_then(Value::as_str);
            Some(entry(id, name, description, efforts, default))
        })
        .collect();
    let effort_id = effort
        .and_then(|option| option.get("id")?.as_str())
        .unwrap_or(EFFORT_ID);
    models_of(model_id, Some(effort_id), entries, current, current_effort)
}

/// A select option's choices, ACP's groups flattened in order.
fn choices(option: &Value) -> Vec<Value> {
    let Some(list) = option.get("options").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .flat_map(|item| match item.get("options").and_then(Value::as_array) {
            Some(group) => group.clone(),
            None => vec![item.clone()],
        })
        .collect()
}

fn named_choices(choices: &[Value]) -> Vec<NamedChoice> {
    choices
        .iter()
        .filter_map(|choice| {
            let id = choice.get("value")?.as_str()?;
            let name = choice.get("name").and_then(Value::as_str).unwrap_or(id);
            Some(named(id, name))
        })
        .collect()
}

/// The levels of a model's AIR `reasoningEfforts`, each named as the effort option names it, else
/// as the adapter names a level (`effortLevelName`, `dist/session-effort.js`).
fn air_efforts(air: &Value, effort_choices: &[Value]) -> Vec<NamedChoice> {
    let known = named_choices(effort_choices);
    air.get("values")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(|level| {
                    known
                        .iter()
                        .find(|choice| choice.id == level)
                        .cloned()
                        .unwrap_or_else(|| named(level, &level_name(level)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `x_high` as "X High": each part split at `_` or `-`, capitalised.
fn level_name(level: &str) -> String {
    level
        .split(['_', '-'])
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}
