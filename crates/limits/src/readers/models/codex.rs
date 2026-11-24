//! Codex's model probe: `codex app-server` as the limits reader runs it (Clauseo's own
//! `CODEX_HOME` and its `-c` overrides), `initialize`, then every page of `model/list`. Names
//! follow codex-acp 2.10.0, so the pickers read as a session's options do: a `gpt-` display
//! name loses the prefix and each part is capitalised (`formatModelDisplayName`,
//! `dist/index.js:37462-37465`), an effort is its value capitalised (`:37532`), and the option
//! ids are `model` and `reasoning_effort` (`:37460-37461`).

use std::path::Path;
use std::time::Duration;

use clauseo_codex_appserver::{ClientInfo, Model, connect};
use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentModels;
use clauseo_settings::AgentLaunch;
use futures::future::join;
use futures::{AsyncRead, AsyncWrite};

use super::super::codex::{Prepared, prepare};
use super::super::run::{Ran, Stdout, run};
use super::{MAX_PAGES, entry, models_of, named};

/// How long the probe may take before its tree is killed, as a limits read.
pub(crate) const LIMIT: Duration = Duration::from_secs(10);
const MODEL_ID: &str = "model";
const EFFORT_ID: &str = "reasoning_effort";

/// One probe of Codex's models; `None` on any failure, a missing home included.
pub(crate) async fn probe<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    launch: &AgentLaunch,
    run_dir: &Path,
    codex_home: &Path,
) -> Option<AgentModels> {
    let spec = match prepare(launch, run_dir, codex_home, std::env::vars_os()) {
        Ok(Prepared::Spec(spec)) => spec,
        Ok(Prepared::SignedOut) => return None,
        Err(message) => {
            tracing::info!(%message, "Codex's model probe did not launch");
            return None;
        }
    };
    let ran = run(
        launcher,
        &spec,
        timer,
        LIMIT,
        |stdin, stdout| async move {
            let listed = talk(stdin, stdout).await;
            let rest: Stdout = Box::new(futures::io::empty());
            (listed, rest)
        },
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
        _ => {
            tracing::info!("Codex's model probe got no models");
            None
        }
    }
}

async fn talk<W, R>(stdin: W, stdout: R) -> Option<AgentModels>
where
    W: AsyncWrite + Send + Unpin + 'static,
    R: AsyncRead + Send + Unpin + 'static,
{
    let (client, notifications, driver) = connect(stdin, stdout);
    drop(notifications);
    let calls = async move {
        client.initialize(ClientInfo::clauseo()).await?;
        client.all_models(MAX_PAGES).await
    };
    let (_driven, listed) = join(driver, calls).await;
    models(&listed.ok()?)
}

/// The models of `model/list`'s pages: the visible ones, the current the one Codex marks
/// default (else the first), at its default effort.
pub(crate) fn models(listed: &[Model]) -> Option<AgentModels> {
    let visible: Vec<&Model> = listed.iter().filter(|model| !model.hidden).collect();
    let entries = visible
        .iter()
        .map(|model| {
            let efforts = model
                .supported_reasoning_efforts
                .iter()
                .map(|option| {
                    let value = option.reasoning_effort.as_str();
                    named(value, &capitalised(value))
                })
                .collect();
            let name = display_name(&model.display_name);
            let default = Some(model.default_reasoning_effort.as_str());
            entry(&model.id, &name, Some(&model.description), efforts, default)
        })
        .collect();
    let current = visible.iter().find(|model| model.is_default);
    let current_id = current.map(|model| model.id.as_str());
    models_of(MODEL_ID, Some(EFFORT_ID), entries, current_id, None)
}

/// codex-acp's `formatModelDisplayName`: `gpt-6.2-sol` as "6.2 Sol"; any other name as it is.
fn display_name(name: &str) -> String {
    let Some(rest) = name
        .get(..4)
        .filter(|prefix| prefix.eq_ignore_ascii_case("gpt-"))
        .and_then(|_| name.get(4..))
    else {
        return name.to_owned();
    };
    rest.split(['-', '/'])
        .filter(|part| !part.is_empty())
        .map(capitalised)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalised(value: &str) -> String {
    let mut chars = value.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
