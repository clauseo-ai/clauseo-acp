//! The set-aside notices: what the seam moved out of the matter before a launch
//! (`before_launch`) or after a session (`after_session`), named relative to the matter, one
//! `Notice` each. A name outside the matter is dropped, never logged; where a name went (`to`)
//! is never shown.

use std::path::{Component, Path, PathBuf};

use clauseo_protocol::{Notice, NoticeLevel, StreamId, ThreadId};
use clauseo_sandbox::SetAside;

use super::super::super::super::Actor;
use super::super::super::Then;
use super::super::super::out::Out;
use crate::error::Error;
use crate::log_file::Cause;

/// The most set-aside names a notice lists; the rest are counted.
pub(super) const NAMED: usize = 10;

/// When the names were moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum When {
    /// Before the agent started (`before_launch`).
    BeforeLaunch,
    /// After its session ended (`after_session`).
    AfterSession,
}

/// The names in `set_aside` relative to `matter` (the policy's matter root), `/`-joined: each
/// `from` (absolute, maybe with Windows' `\\?\` prefix) loses that prefix, then the root. One
/// outside the matter, or naming the matter itself, is dropped.
pub(super) fn relative_names(set_aside: &[SetAside], matter: &Path) -> Vec<String> {
    let root = plain(matter);
    set_aside
        .iter()
        .filter_map(|moved| {
            let from = plain(&moved.from);
            let relative = from.strip_prefix(&root).ok()?;
            let parts = relative
                .components()
                .map(|part| match part {
                    Component::Normal(name) => Some(name.to_string_lossy()),
                    Component::Prefix(_)
                    | Component::RootDir
                    | Component::CurDir
                    | Component::ParentDir => None,
                })
                .collect::<Option<Vec<_>>>()?;
            (!parts.is_empty()).then(|| parts.join("/"))
        })
        .collect()
}

/// `path` without Windows' verbatim prefix: `\\?\C:\m` is `C:\m`, `\\?\UNC\srv\share` is
/// `\\srv\share`. Any other path is as it is.
fn plain(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_owned();
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    text.strip_prefix(r"\\?\")
        .map_or_else(|| path.to_owned(), PathBuf::from)
}

/// The notice of the names `moved` (relative to the matter), logged on the thread once its
/// stream is loaded: at most [`NAMED`] named, the rest counted.
pub(super) fn log(actor: &mut Actor, thread: ThreadId, moved: &[String], when: When) {
    tracing::info!(%thread, moved = moved.len(), ?when, "the sandbox set protected names aside");
    let notice = notice(moved, when);
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let logged = loaded.and_then(|()| {
                let mut out = Out::new(actor, thread, None)?;
                out.push(actor, notice)?;
                out.send(actor, Then::Nothing)
            });
            if let Err(error) = logged {
                let error = Cause(&error);
                tracing::warn!(%thread, %error, "a set-aside notice was not logged");
            }
            actor.release(stream);
        }),
    );
}

/// The set-aside notice's text and level. No `NoticeKind` yet (that is a wire change, R17
/// PR-D/E): the text is the notice.
pub(super) fn notice(moved: &[String], when: When) -> Notice {
    let named = moved
        .iter()
        .take(NAMED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let more = moved.len().saturating_sub(NAMED);
    let what = match moved.len() {
        1 => "a protected item that changed".to_owned(),
        n => format!("{n} protected items that changed"),
    };
    let rest = if more == 0 {
        String::new()
    } else {
        format!(" and {more} more")
    };
    let moment = match when {
        When::BeforeLaunch => "Before the agent started",
        When::AfterSession => "After the agent's session ended",
    };
    Notice {
        level: NoticeLevel::Warning,
        text: format!(
            "{moment}, Clauseo moved {what} in this matter to a safe place: {named}{rest}."
        ),
        kind: None,
    }
}
