# 0063: "Always for this matter", kept by Clauseo for both agents

## Context

A lawyer should never have to approve the same thing twice. Today the card's "Always for this
matter" sends the agent's own allow-always option, and each agent keeps something different:

- Codex 0.162.1 writes an unsandboxed prefix rule into `CODEX_HOME/rules/default.rules`. That
  home is shared by every matter, so the rule applies everywhere, for good.
- claude-agent-acp 0.89.1 writes Claude Code's suggested rule into
  `<matter>/.claude/settings.local.json`, which a lawyer's own Claude Code reads later.

Other options labelled "Always" last for one session only (Codex `allow_for_session`,
`allow_session`; Claude's file `allow-with-updates`). In the census of 40 real cards
(stage C, `approval-census.md`), "Always" was chosen once. Windows never offered it.

#1109 (`claude-always`) already moved Claude's "Always" into Clauseo:
`<data>/agent-state/<matter>/claude-allow.json`, answered by the host after its checks, with a
list and Remove. This record extends that one store to both agents, and changes what it keeps.

## Decision

**One store.** #1109's file, `<data>/agent-state/<matter>/claude-allow.json`, holds every entry.
The host reads it, matches entries and answers. No agent reads it: both fences deny it (Claude
by the fixed pattern `agent-state/*/claude-allow.json`; Codex by #1109's deny of the matter
folder's entries). It never reaches an agent's settings. Version 2 of the file adds an
**HMAC-SHA256** over the matter id and every rule. The key is 32 random bytes in the OS secret
store (`SecretStore`: the macOS Keychain, Windows Credential Manager, the same seam the desktop
uses for the block list; `MemoryStore` in tests). Account: `always-allowed-hmac`.

- A file without a valid HMAC is set aside as `claude-allow.json.bad-<n>` at the next write and
  holds nothing. That covers a file edited by hand, planted, copied from another matter (the
  matter id is in the MAC), or a version-1 file.
- Each rule read back is checked again for eligibility (below). A rule that fails is dropped.
- A whole-file MAC also fails closed on a rule removed by editing. An old copy of the file put
  back is accepted (no counter). Only a process that can write `<data>` can do that, and
  neither agent can.
- No secret store (Linux, a Keychain "Deny", any error): nothing is kept, and the card offers
  no "Always". Why a whole-file MAC rather than one per rule: it is one value, one check, and it
  catches a rule dropped or moved between files.

**Lifecycle.** The file is deleted when the matter is forgotten. A move keeps the matter id, so
the file follows it. A duplicate gets a new id and starts empty.

**What an entry is.** Rule text in Clauseo's own syntax, never sent to an agent:

| Kind | Rule | Agent | Matches |
|---|---|---|---|
| command | `Bash(<cmd>)`, `PowerShell(<cmd>)` | Claude | the exact command text the card showed |
| command | `Run(<cmd>)`, `Run(<cmd>) in <cwd>` | Codex | the exact command text and the working folder relative to the matter |
| file edit | `Edit(*)` | both | any change to a file whose real path is inside the matter folder |
| site | `WebFetch(domain:<host>)` | Claude | a WebFetch, or a `SandboxNetworkAccess` card, for that exact host |
| tool | `mcp__<server>__<tool>` | Claude | that server's tool |
| tool | `WebSearch` | Claude | every web search in the matter (all or nothing: a search has no site) |
| read | `Read(//<path>)` | Claude | #1109's per-file read, unchanged |

Commands are kept per agent. Codex runs an approved escalation outside its sandbox, so a
command Claude was allowed to run does not answer a Codex card.

**What a command must be to be kept.** The key is the command text the card shows, byte for
byte. Until permission-shown lands, that text comes from `always::shown_command`, a copy of its
builder; it swaps to the real field on rebase. The text must parse as **one plain command**:

- Words with plain characters, `'…'` and `"…"` without `$`, a backquote, `\` or `!` inside.
  Anything else fails closed: `; & | < > ( ) $ \` \ ! ^ * ? [ ] { } ~ #`, a line break, a
  control or invisible character, or a first word with `=` or a `/`.
- The program is not on the wrapper list (`always/command.rs`, `WRAPPERS`): shells,
  interpreters, program runners, build and package tools, network tools, editors and pagers,
  privilege tools, and `git`, whose config and hooks can run programs. `find` with `-exec`,
  `-execdir`, `-ok`, `-okdir`, `-delete` or `-fprint*`, and `sed -f`, are refused too.
- No argument is an absolute path outside the matter, or leads out of it with `..`.
- Agents' own wrappers: codex-acp shows `stripShellPrefix(shlex_join(argv))`. For
  `[zsh, -lc, script]` that is the script. A shown `bash|sh|zsh -c|-lc <script>` form is read
  through once, for Codex only, and its script must itself be one plain command. Claude's Bash
  string is the script. A `bash -c` the model writes is a wrapper.
- PowerShell (Claude's tool, Codex's `powershell -Command`): words of letters, digits and
  `- _ . / \ : , = +`, or `'…'`. No `$`, a backquote, `;`, `|`, `&`, `(`, `{`, `@`, `>`.
  Cmdlets that run other code (`Invoke-Expression`, `Start-Process`, `Invoke-Command`,
  `Set-Alias`, `Add-Type`, …) are on the list.

**File edits.** One `Edit(*)` per matter covers every file whose real path is inside the
matter folder. The matter thread resolves the path the same way the held-open check does:
links and junctions followed, `..` resolved, case folded on Windows and macOS, trailing dots and
spaces dropped on Windows. A network path in its UNC form is compared after the same
resolution. The `.clauseo` and `.claude` folders are never inside. A path that fails to resolve
asks.

**Answering.** A later card matching an entry is answered by the host after the data-fence and
held-open checks, with a **one-time** approval:

- Codex: `allow_once` (`Approved`).
- Claude: `allow-once`.

It is logged as `PermissionRequested` and `PermissionResolved` in one commit
(`by: local_desktop`). The log line holds the thread id, the agent kind, the entry kind and
nothing of the command (0047).

An entry answers only threads whose matter is its matter (`threads.matter_id`). A thread that
reads another matter's files under an invite still uses only its own matter's entries.

**Options the host sends.** An agent option is dropped on arrival when it would write a rule or
claims a scope it doesn't have:

- Codex: `accept_execpolicy_amendment`, `apply_network_policy_amendment:*`, `allow_for_session`,
  and MCP `allow_always`.
- Claude: `allow-with-updates`, `allow-skill-exact`, `allow-skill-prefix`.
- Any option whose id starts with `clauseo.`.

Two host options take their place:

- `clauseo.always` (kind `allow_always`), added when the request is eligible. Answering it keeps
  the entry and sends the agent its allow-once option.
- `clauseo.session:<agent id>`: an agent's session-only option (Codex MCP `allow_session`),
  shown as "For this conversation". Answering it sends the agent's own id.

Plan-exit options pass unchanged.

**Forced ask (Windows network or unprotected folders, 0030).** The store applies there too. The
host also answers, with no card, the tools of servers it injected itself
(`asks::bundled_tool_to_allow`): case law's `search_law`, `read_document`, `citator`,
`check_citations` and `account`, by exact name, on the reserved `case-law` server Clauseo sent.
`recharge` and any tool that spends money always asks.

**Protocol.** `PermissionRequested.always: Option<AlwaysOffer { kind, shown, cwd }>` tells the
card what "Always" would keep. #1109's `HostQuery::KeptPermissions` and
`HostCommand::RemoveKeptPermission` list and remove entries, and `KeptKind` gains `command` and
`file_edit` meanings for the new rules (below).

### Proposed words (provisional, for the wording page)

| Id | Where | Words |
|---|---|---|
| UC-810 | card, command | Always allow this exact command in {folder}. (`{folder}` = cwd, or "this matter's folder") |
| UC-811 | card, file edit | Always allow changes to files in this matter. |
| UC-812 | card, site | Always allow {site} in this matter. |
| UC-813 | not saved | Clauseo couldn't save this, so it applies only until Clauseo closes. |
| UC-814 | thread, host answered | Allowed: always for this matter. |
| UC-815 | thread, "No" holds | You said no to this before. {agent} asked again, so Clauseo said no for you. |
| UC-816 | empty list | Nothing is always allowed in this matter yet. |
| UC-817 | session option | For this conversation |

The list reuses #1109's "Kept permissions…" dialog and `UBL-26` "Remove".

### What still asks every time

| Case | Example | Census |
|---|---|---|
| A command with a shell, interpreter or runner | `swift read.swift Brief.pdf`, `python3 x.py`, `bash -c "…"`, `npx …` | S5:22, S5:32 |
| A chain, pipe, redirection or substitution | `cd sub && ls`, `ls > list.txt`, `cat $(ls)` | |
| A command outside the matter | `mv ../Q ../Q2` | S249:25 |
| A command with a web address (Codex) | `curl -L -o judgment.pdf https://…` (refused unasked, `shell_network.rs`) | S3:48, S22:63 |
| Network approvals for Codex | any host (Codex sites: `docs/notes/codex-network-sites.md`) | |
| A Codex command with a backslash | `type C:\m\a.txt` (Codex on Windows) | |
| An ordinary command in a new folder | `mdls a.pdf` in `Drafts` after it was allowed in the matter folder | S5:33 |
| `git` | `git status` (hooks and config can run programs) | |
| A file outside the matter | an edit of `../B/notes.txt` | row 10 |
| A Codex MCP tool of a lawyer's server not trusted | asked once per conversation ("For this conversation") | |
| `recharge` | case law's credit purchase | |
| No secret store | Linux, or a refused Keychain prompt | row 21 |
| A command longer than 8 KiB, cut, or holding `[withheld]` | | |

## Consequences

- Neither agent writes a rule anywhere, so nothing outlives the matter or leaks across matters.
- The parser fails closed. Ordinary harmless commands written with a pipe or a quote it refuses
  (`ls | head`) still ask every time. That is the cost of exact, fail-closed matching.
- Removing an entry takes effect on the actor at once, before the file write (#1109's ledger).
- Codex network sites are a later PR (`docs/notes/codex-network-sites.md`).

## Status

proposed

## Date

2026-11-16
