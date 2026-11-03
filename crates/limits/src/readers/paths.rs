//! Where a reader's files are: the adapter's install folder, found from its settings entry, and
//! the no-symlink rule every path under that folder passes.
//!
//! The install folder is the parent of the nearest `node_modules` folder above the adapter's
//! script (`args[0]`). The folder itself and its ancestors may be links, since that is where the
//! user put it; nothing from it down to a file the reader uses may be. That refuses pnpm and
//! linked installs, which put links under `node_modules`: they read as "no limits" until stage D's
//! `agents` owns the install.
//!
//! Claude's install folder must hold the SDK the helper imports,
//! `node_modules/@anthropic-ai/claude-agent-sdk/package.json`, by the same rule. Without it node
//! would climb to any parent `node_modules` and run some other SDK. The entry file that
//! `package.json` names (`entry.rs` says how) passes the rule too, so a linked `sdk.mjs` cannot
//! load code from outside the install. The SDK's other files, which the entry imports in turn,
//! are not walked. A global npm install, whose install folder is `<prefix>/lib` with the SDK
//! nested under the adapter, is refused too: the helper could not import the adapter's SDK from
//! there.

use std::io;
use std::path::{Component, Path, PathBuf};

use clauseo_settings::AgentLaunch;

use crate::error::Error;

mod entry;

/// Claude's settings id.
pub(crate) const CLAUDE_ID: &str = "claude-acp";
/// Codex's settings id.
pub(crate) const CODEX_ID: &str = "codex-acp";
/// The helper's file name in Claude's install folder.
pub(crate) const CLAUDE_HELPER: &str = "clauseo-claude-usage.mjs";
/// Where the Claude Agent SDK's `package.json` sits inside a `node_modules` folder.
const CLAUDE_SDK: [&str; 3] = ["@anthropic-ai", "claude-agent-sdk", "package.json"];
/// Where the Codex CLI's script sits inside a `node_modules` folder.
const CODEX_JS: [&str; 4] = ["@openai", "codex", "bin", "codex.js"];

/// An adapter's checked paths: the program, its install folder and its script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Located {
    /// The settings `command` (node), an absolute path.
    pub(crate) program: PathBuf,
    /// The install folder: the parent of the nearest `node_modules` above the script.
    pub(crate) install: PathBuf,
    /// The adapter's script, `args[0]`: a regular file reached through no link.
    pub(crate) script: PathBuf,
}

/// Claude's program and the helper's path in its install folder. The install folder's
/// `node_modules/@anthropic-ai/claude-agent-sdk/package.json`, and each file it names as the
/// entry an `import` loads, must be a regular file reached through no link. The helper may be
/// missing (the read writes it), but when something is there it must be a regular file, not a
/// link or a folder.
pub(crate) fn claude(launch: &AgentLaunch) -> Result<(Located, PathBuf), Error> {
    let located = locate(CLAUDE_ID, launch)?;
    let modules = located.install.join("node_modules");
    let sdk = CLAUDE_SDK
        .iter()
        .fold(modules.clone(), |path, part| path.join(part));
    match walk(&located.install, &sdk) {
        Ok(()) => {}
        Err(Walk::Missing) => {
            let rule = format!(
                "no @anthropic-ai/claude-agent-sdk/package.json in {}",
                modules.display()
            );
            return Err(Error::Refused {
                agent: CLAUDE_ID,
                rule,
            });
        }
        Err(Walk::Refused(rule)) => {
            return Err(Error::Refused {
                agent: CLAUDE_ID,
                rule,
            });
        }
    }
    for file in entry::entry_files(&sdk).map_err(|rule| Error::Refused {
        agent: CLAUDE_ID,
        rule,
    })? {
        match walk(&located.install, &file) {
            Ok(()) => {}
            Err(Walk::Missing) => {
                return Err(refused(
                    CLAUDE_ID,
                    &file,
                    "(the SDK's entry in its package.json) does not exist",
                ));
            }
            Err(Walk::Refused(rule)) => {
                return Err(Error::Refused {
                    agent: CLAUDE_ID,
                    rule,
                });
            }
        }
    }
    let helper = located.install.join(CLAUDE_HELPER);
    match std::fs::symlink_metadata(&helper) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => {
            return Err(refused(
                CLAUDE_ID,
                &helper,
                "exists and is not a regular file",
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(refused(
                CLAUDE_ID,
                &helper,
                &format!("does not read: {err}"),
            ));
        }
    }
    Ok((located, helper))
}

/// The Codex CLI's `codex.js`: codex-acp's own `node_modules/@openai/codex/bin/codex.js`
/// first, then the install folder's, each by the no-symlink rule. A candidate that is missing
/// moves on to the next; one that breaks the rule refuses.
pub(crate) fn codex_js(launch: &AgentLaunch) -> Result<(Located, PathBuf), Error> {
    let located = locate(CODEX_ID, launch)?;
    let modules = located.install.join("node_modules");
    let package = package_dir(&modules, &located.script);
    let own = package
        .as_deref()
        .map(|package| package.join("node_modules"));
    for dir in own.iter().chain(std::iter::once(&modules)) {
        let candidate = CODEX_JS
            .iter()
            .fold(dir.clone(), |path, part| path.join(part));
        match walk(&located.install, &candidate) {
            Ok(()) => return Ok((located, candidate)),
            Err(Walk::Missing) => {}
            Err(Walk::Refused(rule)) => {
                return Err(Error::Refused {
                    agent: CODEX_ID,
                    rule,
                });
            }
        }
    }
    let rule = format!(
        "no @openai/codex/bin/codex.js in codex-acp's node_modules or in {}",
        modules.display()
    );
    Err(Error::Refused {
        agent: CODEX_ID,
        rule,
    })
}

/// Checks the readers' run folder `<data_dir>/limits/run` and its parent `limits`: each may be
/// missing (the read makes it), but when there it must be a folder, not a link or a file.
pub(crate) fn check_run_dir(run_dir: &Path) -> Result<(), Error> {
    for dir in run_dir.parent().into_iter().chain(std::iter::once(run_dir)) {
        let what = match std::fs::symlink_metadata(dir) {
            Ok(meta) if meta.is_dir() => continue,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Ok(meta) if meta.file_type().is_symlink() => "is a link".to_owned(),
            Ok(_) => "is not a folder".to_owned(),
            Err(err) => format!("does not read: {err}"),
        };
        return Err(Error::RunFolder {
            path: dir.to_owned(),
            what,
        });
    }
    Ok(())
}

/// Checks Clauseo's Codex home with `symlink_metadata`, reading nothing inside it: `true` for a
/// folder, `false` when it is missing, and a refusal for a relative path, a link or anything
/// else. A relative home is refused first: it would be checked against Clauseo's working folder,
/// while codex resolves it against the run folder.
pub(crate) fn check_codex_home(codex_home: &Path) -> Result<bool, Error> {
    if !codex_home.is_absolute() {
        return Err(Error::CodexHome {
            path: codex_home.to_owned(),
            what: "is not an absolute path".to_owned(),
        });
    }
    let what = match std::fs::symlink_metadata(codex_home) {
        Ok(meta) if meta.is_dir() => return Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Ok(meta) if meta.file_type().is_symlink() => "is a link".to_owned(),
        Ok(_) => "is not a folder".to_owned(),
        Err(err) => format!("does not read: {err}"),
    };
    Err(Error::CodexHome {
        path: codex_home.to_owned(),
        what,
    })
}

/// Checks `launch` and finds its install folder.
pub(crate) fn locate(agent: &'static str, launch: &AgentLaunch) -> Result<Located, Error> {
    let program = PathBuf::from(&launch.command);
    if !program.is_absolute() {
        return Err(refused(
            agent,
            &program,
            "(command) is not an absolute path",
        ));
    }
    let Some(script) = launch.args.first() else {
        return Err(Error::Refused {
            agent,
            rule: "args is empty; args[0] must be the adapter's script".to_owned(),
        });
    };
    let script = PathBuf::from(script);
    if !script.is_absolute() {
        return Err(refused(agent, &script, "(args[0]) is not an absolute path"));
    }
    if has_dot_part(&script) {
        return Err(refused(agent, &script, "(args[0]) has a . or .. part"));
    }
    let Some(install) = script
        .ancestors()
        .skip(1)
        .find(|dir| {
            dir.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("node_modules"))
        })
        .and_then(Path::parent)
    else {
        return Err(refused(
            agent,
            &script,
            "(args[0]) has no node_modules folder above it",
        ));
    };
    let install = install.to_owned();
    match walk(&install, &script) {
        Ok(()) => Ok(Located {
            program,
            install,
            script,
        }),
        Err(Walk::Missing) => Err(refused(agent, &script, "(args[0]) does not exist")),
        Err(Walk::Refused(rule)) => Err(Error::Refused { agent, rule }),
    }
}

/// Whether `path`, as written, has a `.` or `..` part. `Path::components` drops an inner `.`,
/// so the text is split instead.
fn has_dot_part(path: &Path) -> bool {
    path.as_os_str()
        .as_encoded_bytes()
        .split(|&byte| byte == b'/' || (cfg!(windows) && byte == b'\\'))
        .any(|part| part == b"." || part == b"..")
}

/// The adapter's package folder: the folder right under `modules` that holds `script`, with its
/// scope (`@agentclientprotocol/claude-agent-acp`).
fn package_dir(modules: &Path, script: &Path) -> Option<PathBuf> {
    let mut parts = script.strip_prefix(modules).ok()?.components();
    let Some(Component::Normal(first)) = parts.next() else {
        return None;
    };
    let mut package = modules.join(first);
    if first.as_encoded_bytes().starts_with(b"@")
        && let Some(Component::Normal(name)) = parts.next()
    {
        package.push(name);
    }
    Some(package)
}

/// Why [`walk`] refused a path.
enum Walk {
    /// A part of it does not exist.
    Missing,
    /// It breaks a rule; the text names it.
    Refused(String),
}

/// Checks each part of `file` below `install` with `symlink_metadata`: none may be a link, each
/// folder must be a folder and `file` a regular file. `install` itself is not checked.
fn walk(install: &Path, file: &Path) -> Result<(), Walk> {
    let rest = file.strip_prefix(install).map_err(|err| {
        Walk::Refused(format!(
            "{} is not inside {}: {err}",
            file.display(),
            install.display()
        ))
    })?;
    let mut path = install.to_owned();
    let mut parts = rest.components().peekable();
    while let Some(part) = parts.next() {
        let Component::Normal(part) = part else {
            return Err(Walk::Refused(format!(
                "{} has a part that is not a name",
                file.display()
            )));
        };
        path.push(part);
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Err(Walk::Missing),
            Err(err) => {
                return Err(Walk::Refused(format!(
                    "{} does not read: {err}",
                    path.display()
                )));
            }
        };
        let last = parts.peek().is_none();
        if meta.file_type().is_symlink() {
            return Err(Walk::Refused(format!(
                "{} is a link inside the install folder {}",
                path.display(),
                install.display()
            )));
        }
        if last && !meta.is_file() {
            return Err(Walk::Refused(format!(
                "{} is not a regular file",
                path.display()
            )));
        }
        if !last && !meta.is_dir() {
            return Err(Walk::Refused(format!("{} is not a folder", path.display())));
        }
    }
    Ok(())
}

/// An [`Error::Refused`] for `agent`: `path` and what is wrong with it.
fn refused(agent: &'static str, path: &Path, what: &str) -> Error {
    Error::Refused {
        agent,
        rule: format!("{} {what}", path.display()),
    }
}

#[cfg(test)]
mod tests;
