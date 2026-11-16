//! "Always for this matter", kept by Clauseo for both agents (decision record 0063): the host
//! keeps the rule itself, never an agent, and answers a later matching card with a one-time
//! approval.
//!
//! Why: each agent's own "always" writes a rule somewhere an agent or the lawyer's own tools
//! read later (Codex an unsandboxed prefix rule in its shared home; claude-agent-acp 0.89.1
//! Claude Code's suggestion into `<matter>/.claude/settings.local.json`). So the turn path drops
//! every rule-writing option, offers its own `clauseo.always`, answers the agent with the
//! request's allow-once option (`actor/turn/always.rs`) and keeps the rule here:
//!
//! - [`rule`]: the narrowest rule for the request, in Clauseo's own syntax, from the agent, the
//!   tool's name or kind and `rawInput` alone (the table in 0063). Anything else, and any text
//!   that would not stay one exact rule, gives none, with why ([`NotKept`]).
//! - [`within`] and [`real_within`]: a path rule is kept and applied only for a file inside the
//!   matter folder, by its text and by its real path (links resolved), never in `.clauseo` or a
//!   `.claude` folder.
//! - The file, `<data>/agent-state/<matter>/claude-allow.json` ([`file_beside`], [`store`]),
//!   beside the matter's notes and not in them, in the data fence's fixed deny rules
//!   (`data_fence::DENIED`). Nothing is ever written in the matter folder.
//!
//! The kept rules never reach an agent's settings: the agent would then allow a matching call
//! itself, past the host's data fence, its held-open check and its log.

use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, AlwaysKind, AlwaysOffer, ToolKind};
use serde_json::Value;

pub(crate) use site::kept_sites;
use site::{domain, domain_name, sandbox_host};

pub(crate) mod options;
pub(crate) mod plain;
mod reach;
mod real;
mod site;
mod store;
pub(crate) mod table;

pub(crate) use reach::{Program, Reach};
use real::{name_as_opened, own_folder};
pub(crate) use real::{real_inside, real_within};

pub(crate) use store::{delete, list, next_seq, read_beside, remove_now, store, use_secrets};
#[cfg(test)]
pub(crate) use store::{read, store_now};

/// The file's name, in `<data>/agent-state/<matter>`.
pub(crate) const FILE: &str = "claude-allow.json";

/// The most rules a matter keeps; an answer past it keeps nothing more.
pub(crate) const MAX_RULES: usize = 500;

/// The longest rule kept, in bytes.
const MAX_RULE_LEN: usize = 2_000;

/// Why "Always for this matter" kept nothing: the request was allowed this once only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotKept {
    /// The tool has no rule form Clauseo keeps (a shell other than Bash, a tool it does not know).
    NoRule,
    /// Its command, address or path would not stay one exact rule (a wildcard, a bracket, a line
    /// break, a relative path).
    Refused,
    /// A file outside the matter folder, or in its `.clauseo` or a `.claude` folder.
    OutsideMatter,
    /// The matter keeps the most rules it may (`MAX_RULES`).
    Full,
}

/// A permission request as [`rule`] reads it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Request<'a> {
    /// Whose request it is: commands are kept per agent.
    pub(crate) agent: AgentKind,
    /// Claude Code's tool name (`_meta.claudeCode.toolName`); none for Codex.
    pub(crate) tool: Option<&'a str>,
    /// The request's `toolCall.kind`: Codex's command (`execute`) and file-change (`edit`)
    /// cards are read by it.
    pub(crate) kind: Option<ToolKind>,
    /// `toolCall.rawInput`.
    pub(crate) raw_input: Option<&'a Value>,
    /// The matter folder's spellings, as text: the folder the agent was given first, then the
    /// others it is known by (a link resolved, `/var` and `/private/var` on macOS).
    pub(crate) matter: &'a [String],
    /// Windows paths.
    pub(crate) windows: bool,
}

/// What "Always for this matter" would keep from a request: its rule, and what the card says
/// it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kept {
    pub(crate) rule: String,
    pub(crate) offer: AlwaysOffer,
    /// A command rule's program, working folder and path words, which the matter thread checks
    /// by real path at keep time and at every use (`reach.rs`); none for any other rule.
    pub(crate) command: Option<Command>,
}

/// What a command rule's request runs, for its real-path check (`Reach`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Command {
    /// Its program's entry in the allowlist (`table.rs`).
    pub(crate) program: &'static table::Spec,
    /// Its working folder, absolute, as text: Codex's `cwd`; for Claude the matter folder,
    /// where Clauseo has its shell start every call (`loads::CLAUDE_SHELL_FOLDER_VAR`).
    pub(crate) cwd: String,
    /// Its path words as typed, absolute or relative to `cwd`.
    pub(crate) paths: Vec<String>,
    /// Whether it is Codex's: codex-acp shows `bash <name>` as `<name>`, so no entry of the
    /// working folder may bear the program's name.
    pub(crate) codex: bool,
}

/// The one rule that covers every change to a file inside the matter folder.
pub(crate) const EDIT_ALL: &str = "Edit(*)";

/// Tools that spend the lawyer's money: each call asks (decision record 0063).
const SPENDS: &[&str] = &["mcp__case-law__recharge"];

/// Whether the MCP tool `server`'s `tool` spends the lawyer's money ([`SPENDS`]): its card
/// never offers "Always" nor "For this conversation".
pub(crate) fn spends(server: &str, tool: &str) -> bool {
    SPENDS.contains(&format!("mcp__{server}__{tool}").as_str())
}

/// Claude's web search, kept whole: a search names no site.
pub(crate) const WEB_SEARCH: &str = "WebSearch";

/// The rule a request keeps, and what the card says it covers, or why none (decision 0063):
///
/// - Claude: `Bash(<cmd>)` and `PowerShell(<cmd>)` for one plain command (`plain.rs`) whose
///   arguments stay in the matter; `WebFetch(domain:<host>)`; `WebSearch`; an MCP tool's exact
///   name; `Read(//<path>)`; `Edit(*)` for `Edit`, `Write`, `MultiEdit` and `NotebookEdit`.
/// - Codex: `Run(<cmd>)`, or `Run(<cmd>) in <cwd>` when it runs in a folder of the matter, for
///   an `execute` card (`rawInput` `{command, cwd}`, `command` the text the card shows) that
///   asks for no network; `Edit(*)` for an `edit` card.
///
/// `Edit(*)` is kept and applied only when every file the request names is inside the matter by
/// its real path (`real_within`, on the matter thread).
pub(crate) fn rule(request: &Request<'_>) -> Result<Kept, NotKept> {
    let kept = match request.agent {
        AgentKind::Claude => claude_rule(request)?,
        AgentKind::Codex => codex_rule(request)?,
    };
    if kept.rule.len() <= MAX_RULE_LEN {
        Ok(kept)
    } else {
        Err(NotKept::Refused)
    }
}

fn offer(kind: AlwaysKind, shown: Option<String>, cwd: Option<String>) -> AlwaysOffer {
    AlwaysOffer {
        kind,
        shown,
        cwd,
        server: None,
        tool: None,
    }
}

/// A tool's offer: its server and its name there, never the whole `mcp__<server>__<tool>`, so
/// the card can say "{tool} from {server}". Web search has no server.
fn tool_offer(server: Option<&str>, tool: &str) -> AlwaysOffer {
    AlwaysOffer {
        server: server.map(str::to_owned),
        tool: Some(tool.to_owned()),
        ..offer(AlwaysKind::Tool, None, None)
    }
}

fn claude_rule(request: &Request<'_>) -> Result<Kept, NotKept> {
    let raw_input = request.raw_input;
    let text = |key: &str| {
        raw_input
            .and_then(|input| input.get(key)?.as_str())
            .ok_or(NotKept::Refused)
    };
    let exact = |made: Option<String>| made.ok_or(NotKept::Refused);
    let shell = |tool: &str, grammar: plain::Grammar| -> Result<Kept, NotKept> {
        let line = text("command")?;
        let line = exact(command(line, grammar, plain::Agent::Claude).map(str::to_owned))?;
        // Plain in the matter under any of its spellings (Claude's Bash starts in the folder).
        let mut checked = Err(NotKept::OutsideMatter);
        for matter in request.matter {
            let place = plain::Place {
                matter,
                windows: request.windows,
            };
            checked = plain::plain(&line, grammar, plain::Agent::Claude, place)
                .map(|plain| (matter, plain))
                .map_err(not_kept);
            if checked.is_ok() {
                break;
            }
        }
        let (matter, plain) = checked?;
        let command = Command {
            program: plain.program,
            cwd: matter.clone(),
            paths: plain.paths,
            codex: false,
        };
        Ok(Kept {
            rule: format!("{tool}({line})"),
            offer: offer(AlwaysKind::Command, Some(line), None),
            command: Some(command),
        })
    };
    let kept = |rule: String, kind: AlwaysKind, shown: Option<String>| Kept {
        offer: offer(kind, shown, None),
        rule,
        command: None,
    };
    Ok(match request.tool.ok_or(NotKept::NoRule)? {
        "Bash" => shell("Bash", plain::Grammar::Posix)?,
        "PowerShell" => shell("PowerShell", plain::Grammar::PowerShell)?,
        "WebFetch" => {
            let host = exact(domain(text("url")?))?;
            kept(
                format!("WebFetch(domain:{host})"),
                AlwaysKind::Site,
                Some(host),
            )
        }
        // Claude's shell asking for a host the sandbox does not list: the same entry as a
        // WebFetch of that site (claude-agent-acp 0.89.1 `permissions/options/tools.js:102`).
        "SandboxNetworkAccess" => {
            let host = exact(sandbox_host(text("host")?))?;
            kept(
                format!("WebFetch(domain:{host})"),
                AlwaysKind::Site,
                Some(host),
            )
        }
        "WebSearch" => Kept {
            rule: WEB_SEARCH.to_owned(),
            offer: tool_offer(None, WEB_SEARCH),
            command: None,
        },
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            kept(EDIT_ALL.to_owned(), AlwaysKind::FileEdit, None)
        }
        "Read" => {
            let file = text("file_path")?;
            let rule = format!("Read({})", exact(path(file, request.windows))?);
            // The card shows the file relative to the matter folder, `/`-separated. One whose
            // path is outside it keeps its whole path here, but is never offered: a `Read` rule
            // needs the file's real path inside the matter (`needs_real`).
            let shown = request
                .matter
                .iter()
                .find_map(|matter| relative(matter, file, request.windows))
                .filter(|shown| !shown.is_empty())
                .unwrap_or_else(|| file.to_owned());
            kept(rule, AlwaysKind::Read, Some(shown))
        }
        name if SPENDS.contains(&name) => return Err(NotKept::NoRule),
        name if is_mcp_tool(name) => {
            let (server, tool) = mcp_parts(name).ok_or(NotKept::NoRule)?;
            Kept {
                rule: name.to_owned(),
                offer: tool_offer(Some(server), tool),
                command: None,
            }
        }
        _ => return Err(NotKept::NoRule),
    })
}

fn codex_rule(request: &Request<'_>) -> Result<Kept, NotKept> {
    match request.kind {
        Some(ToolKind::Edit) => Ok(Kept {
            rule: EDIT_ALL.to_owned(),
            offer: offer(AlwaysKind::FileEdit, None, None),
            command: None,
        }),
        Some(ToolKind::Execute) => {
            let input = request.raw_input.ok_or(NotKept::Refused)?;
            // Only a request for the command alone, in its folder: one that also asks for a
            // site (`url`) or for more sandbox permissions (`additionalPermissions`, or any key
            // codex-acp adds later) is asked each time, so a kept rule never grants more than
            // the plain run it was kept for.
            let only_command = input.as_object().is_some_and(|input| {
                input
                    .keys()
                    .all(|key| matches!(key.as_str(), "command" | "cwd"))
            });
            if !only_command {
                return Err(NotKept::NoRule);
            }
            let text = |key: &str| input.get(key)?.as_str();
            let (line, cwd) = text("command").zip(text("cwd")).ok_or(NotKept::Refused)?;
            let grammar = codex_grammar(request.windows);
            let line = command(line, grammar, plain::Agent::Codex).ok_or(NotKept::Refused)?;
            let (matter, folder) = request
                .matter
                .iter()
                .find_map(|matter| Some((matter, relative(matter, cwd, request.windows)?)))
                .ok_or(NotKept::OutsideMatter)?;
            let folder = (!folder.is_empty()).then_some(folder);
            let place = plain::Place {
                matter,
                windows: request.windows,
            };
            // An argument outside the matter refuses (decision record 0063, Deviations): an
            // exact command replayed unsandboxed is a standing hole, as a name inside the
            // matter can later be swapped for a link to another matter's file.
            let plain =
                plain::plain(line, grammar, plain::Agent::Codex, place).map_err(not_kept)?;
            let rule = match &folder {
                Some(folder) => format!("Run({line}) in {folder}"),
                None => format!("Run({line})"),
            };
            let command = Command {
                program: plain.program,
                cwd: cwd.to_owned(),
                paths: plain.paths,
                codex: true,
            };
            Ok(Kept {
                rule,
                offer: offer(AlwaysKind::Command, Some(line.to_owned()), folder),
                command: Some(command),
            })
        }
        _ => Err(NotKept::NoRule),
    }
}

/// The grammar Codex's shown command is in: PowerShell on Windows, a POSIX shell elsewhere.
fn codex_grammar(windows: bool) -> plain::Grammar {
    if windows {
        plain::Grammar::PowerShell
    } else {
        plain::Grammar::Posix
    }
}

fn not_kept(why: plain::NotPlain) -> NotKept {
    match why {
        plain::NotPlain::Outside => NotKept::OutsideMatter,
        plain::NotPlain::Size | plain::NotPlain::Unparsed | plain::NotPlain::NotListed => {
            NotKept::Refused
        }
    }
}

/// `cwd` relative to the `matter` folder, `/`-separated, empty for the folder itself; none for
/// a folder outside it, in its `.clauseo` or a `.claude` folder, or one with a
/// name a rule can't hold (empty, `.`, `..`, a parenthesis, a control character, a space at
/// either end). By text: on Windows `\` is `/` and case is folded.
fn relative(matter: &str, cwd: &str, windows: bool) -> Option<String> {
    let norm = |text: &str| {
        let text = if windows {
            text.replace('\\', "/")
        } else {
            text.to_owned()
        };
        text.trim_end_matches('/').to_owned()
    };
    let (matter, cwd) = (norm(matter), norm(cwd));
    let fold = |text: &str| {
        if windows {
            text.to_lowercase()
        } else {
            text.to_owned()
        }
    };
    if matter.is_empty() || fold(&matter).len() != matter.len() || fold(&cwd).len() != cwd.len() {
        return None;
    }
    let folded = fold(&cwd);
    let rest = folded.strip_prefix(&fold(&matter))?;
    if rest.is_empty() {
        return Some(String::new());
    }
    rest.strip_prefix('/')?;
    let rest = cwd.get(matter.len() + 1..)?;
    let names_ok = rest.split('/').all(|name| {
        !matches!(name, "" | "." | "..")
            && name.trim() == name
            && !own_folder(&name_as_opened(name, windows))
            && !name
                .chars()
                .any(|c| matches!(c, '(' | ')') || c.is_control())
    });
    names_ok.then(|| rest.to_owned())
}

/// The file a path rule names, as an absolute path of this system (`//c/a` is `c:/a` on
/// Windows); none for any other rule.
pub(crate) fn rule_path(rule: &str, windows: bool) -> Option<PathBuf> {
    let content = rule
        .strip_prefix("Read(")
        .and_then(|rest| rest.strip_suffix(')'))?;
    let rest = content.strip_prefix("//")?;
    if windows {
        let (drive, rest) = rest.split_once('/')?;
        Some(PathBuf::from(format!("{drive}:/{rest}")))
    } else {
        Some(PathBuf::from(format!("/{rest}")))
    }
}

/// Whether `rule` applies only once the matter thread found what it reaches inside the matter
/// by real path: a `Read` rule's file, every file of an `Edit(*)` request, and a command's
/// program, working folder and path words (`Reach`).
pub(crate) fn needs_real(rule: &str) -> bool {
    rule == EDIT_ALL || rule_path(rule, false).is_some() || is_command(rule)
}

/// Whether `rule` is a command rule: `Bash(…)`, `PowerShell(…)` or `Run(…)`.
fn is_command(rule: &str) -> bool {
    ["Bash(", "PowerShell(", "Run("]
        .iter()
        .any(|tool| rule.starts_with(tool))
}

/// A Codex `Run` rule's command and working folder.
pub(crate) fn run_parts(rule: &str) -> Option<(&str, Option<&str>)> {
    let (line, rest) = rule.strip_prefix("Run(")?.split_once(')')?;
    match rest {
        "" => Some((line, None)),
        rest => Some((line, Some(rest.strip_prefix(" in ")?))),
    }
}

/// Whether `rule` is one [`rule`] could have made: a kept rule read back is checked again.
pub(crate) fn well_formed(rule: &str) -> bool {
    if rule.len() > MAX_RULE_LEN {
        return false;
    }
    if SPENDS.contains(&rule) {
        return false;
    }
    if is_mcp_tool(rule) || rule == EDIT_ALL || rule == WEB_SEARCH {
        return true;
    }
    if let Some((line, folder)) = run_parts(rule) {
        let grammar = codex_grammar(cfg!(windows));
        let folder_ok = folder.is_none_or(|folder| {
            !folder.is_empty()
                && relative("/m", &format!("/m/{folder}"), false).as_deref() == Some(folder)
        });
        return folder_ok && command(line, grammar, plain::Agent::Codex).is_some();
    }
    let Some((tool, content)) = rule.strip_suffix(')').and_then(|rest| rest.split_once('(')) else {
        return false;
    };
    match tool {
        "Bash" => command(content, plain::Grammar::Posix, plain::Agent::Claude).is_some(),
        "PowerShell" => {
            command(content, plain::Grammar::PowerShell, plain::Agent::Claude).is_some()
        }
        "WebFetch" => content
            .strip_prefix("domain:")
            .is_some_and(|host| domain_name(host) && !host.is_empty()),
        "Read" => content
            .strip_prefix('/')
            .is_some_and(|rest| path(rest, false).as_deref() == Some(content)),
        _ => false,
    }
}

/// A shell command as an exact rule's content: not empty, no space at either end, none of
/// `(`, `)` (they end the rule), `*`, `\` or a control character such as a line break, and one
/// plain command in `grammar`.
fn command(command: &str, grammar: plain::Grammar, agent: plain::Agent) -> Option<&str> {
    let plain = !command.is_empty()
        && command.trim() == command
        && !command
            .chars()
            .any(|c| matches!(c, '(' | ')' | '*' | '\\') || c.is_control())
        && one_plain_command(command, grammar, agent);
    plain.then_some(command)
}

/// Whether `command` is one plain command (`plain.rs`): no chain, pipe, redirection,
/// substitution or wrapper. Read here without the matter folder (a rule read back has none), so
/// no place is judged; the request's own check reads the folder too.
fn one_plain_command(command: &str, grammar: plain::Grammar, agent: plain::Agent) -> bool {
    let anywhere = plain::Place {
        matter: "/",
        windows: false,
    };
    match plain::plain(command, grammar, agent, anywhere) {
        Ok(_) | Err(plain::NotPlain::Outside) => true,
        Err(_) => false,
    }
}

/// An MCP tool's server and its name there: `mcp__<server>__<tool>` split at the first `__`
/// after the server's name (Claude Code's own form).
fn mcp_parts(name: &str) -> Option<(&str, &str)> {
    name.strip_prefix("mcp__")?.split_once("__")
}

/// An MCP tool's exact name, `mcp__<server>__<tool>`: letters, digits, `_` and `-` only.
fn is_mcp_tool(name: &str) -> bool {
    name.strip_prefix("mcp__").is_some_and(|rest| {
        rest.split_once("__")
            .is_some_and(|(server, tool)| !server.is_empty() && !tool.is_empty())
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    })
}

/// An absolute file path as a path rule's content in Claude Code's syntax, `//<path>` (a path
/// from the file system's root; a single `/` would be relative to the settings file). On
/// Windows, `C:\a\b` is `//c/a/b`, as Claude Code turns Windows paths into POSIX ones before it
/// matches. None for a relative path, a UNC path, an empty, `.` or `..` component, or a name
/// with a character that is not literal in Claude Code's gitignore patterns (`*`, `?`, `[`, `]`,
/// `{`, `}`, `!`, `\`), a parenthesis, a control character, or a space at either end; and on
/// Windows a name that ends in `.` (Windows drops trailing dots and spaces, so `.claude.` opens
/// `.claude`).
fn path(path: &str, windows: bool) -> Option<String> {
    let rest = if windows {
        let mut chars = path.chars();
        let (drive, colon) = (chars.next()?, chars.next()?);
        let rest = chars.as_str();
        if !drive.is_ascii_alphabetic() || colon != ':' || !rest.starts_with(['\\', '/']) {
            return None;
        }
        format!("{}{}", drive.to_ascii_lowercase(), rest.replace('\\', "/"))
    } else {
        path.strip_prefix('/')?.to_owned()
    };
    let literal = |name: &str| {
        !matches!(name, "" | "." | "..")
            && name.trim() == name
            && (!windows || !name.ends_with('.'))
            && !name.chars().any(|c| {
                matches!(
                    c,
                    '*' | '?' | '[' | ']' | '{' | '}' | '!' | '\\' | '(' | ')'
                ) || c.is_control()
            })
    };
    rest.split('/').all(literal).then(|| format!("//{rest}"))
}

/// The path rule form (`//<path>`) of each of a matter folder's `spellings`, for [`within`];
/// a spelling that has no such form (relative, or a root) gives none.
pub(crate) fn matter_roots<'a>(
    spellings: impl IntoIterator<Item = &'a str>,
    windows: bool,
) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for spelling in spellings {
        let root = path(spelling.trim_end_matches(['/', '\\']), windows);
        if let Some(root) = root.filter(|root| !roots.contains(root)) {
            roots.push(root);
        }
    }
    roots
}

/// Whether `rule` stays within its matter, so it can never widen past the data fence: a path
/// rule (`Edit`, `Read`) only for a file strictly inside the matter folder (one of `roots`,
/// [`matter_roots`]) and in neither its `.clauseo` folder nor a `.claude` one (Claude's own
/// settings). So none names `<data>` (Clauseo's data folder, other matters' notes and runs),
/// another matter's folder or anything else outside. Those two names match in any case (a Mac's
/// disk folds case); `windows`: the whole path is compared in any case, each name without its
/// trailing dots and spaces. By text only: [`real_within`] resolves links. Any other rule names
/// no path and passes.
pub(crate) fn within(rule: &str, roots: &[String], windows: bool) -> bool {
    let content = ["Read("]
        .iter()
        .find_map(|tool| rule.strip_prefix(tool))
        .and_then(|rest| rest.strip_suffix(')'));
    let Some(content) = content else {
        return true;
    };
    let fold = |text: &str| {
        if windows {
            text.to_lowercase()
        } else {
            text.to_owned()
        }
    };
    let content = fold(content);
    roots.iter().any(|root| {
        let rest = content
            .strip_prefix(&fold(root))
            .and_then(|rest| rest.strip_prefix('/'));
        rest.is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .split('/')
                    .all(|name| !own_folder(&name_as_opened(name, windows)))
        })
    })
}

/// `<notes>/../claude-allow.json`: `<data>/agent-state/<matter>/claude-allow.json`.
pub(crate) fn file_beside(notes: &Path) -> Option<PathBuf> {
    notes.parent().map(|matter| matter.join(FILE))
}

/// `<data>/agent-state/<matter>/claude-allow.json`, as [`file_beside`] the matter's notes.
pub(crate) fn file_of(data: &Path, matter: clauseo_protocol::MatterId) -> PathBuf {
    data.join("agent-state").join(matter.to_string()).join(FILE)
}

#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod tests;
