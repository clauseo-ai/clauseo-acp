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
store (`SecretStore`: the macOS Keychain, Windows Credential Manager, under Clauseo's own
keychain service; `MemoryStore` in tests), given to the host by
`HostConfig::with_always_secrets`. Account: `always-allowed-hmac`. The MAC is over
`"v2\0" + matter id + "\0" + rules joined by "\0"`; the matter id is the name of the folder the
file sits in.

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
| tool | `mcp__<server>__<tool>` | Claude | that server's tool (never case law's `recharge`, which spends money) |
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
- Codex's card: `toolCall.kind` `execute`, `rawInput` `{command, cwd}` where `command` is the
  text the card shows; a card with `rawInput.url` (a network approval) gives no rule. A file
  change is `kind: edit`. The acp crate passes `toolCall.kind` on (`tool_kind`).
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
- Any other option of kind `allow_always` (an agent's "always" we don't know yet keeps
  something somewhere).
- Any option whose id starts with `clauseo.`: dropped first, before forced ask's own filter, so
  no agent can offer `clauseo.always` itself.

The drop runs after the checks that read the agent's own options (codex-acp's browser and
trusted-server answers, the shell-network refusal), and before the card is logged.

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

The list reuses #1109's "Kept permissions…" dialog and `UBL-26` "Remove". The words are in
`ui/app/copy-approvals/UC-always-for-this-matter.json` (UC-810..820; UC-818..820 replace
#1109's UC-502, UC-503 and UC-507, UC-816 replaces UC-505).

Later, with the folder grant (`folder_save`, provisional, Hindi later; the list shows a grant
in the same words):

| Id | Agent | Words |
|---|---|---|
| UC-831 | Claude | Always allow Claude to save new files in {folder} and its subfolders in this matter. |
| UC-833 | ChatGPT | Always allow ChatGPT to create, change and delete files in {folder} and its subfolders in this matter. |

The folder grant (for that work): codex-acp's writable roots are the session's
`additionalDirectories`, fixed at `session/new`, `load` and `resume`. At launch the matter's
`folder_save` entries join `codex_directories` and the profile's write entries, canonicalised,
skipping any folder under a deny; mid-conversation only through a re-sent `session/resume`
between turns, after a spike. Codex has no create-only mode, and Windows unelevated refuses
deny-read policies, so a granted folder is read and write for Codex.

Spike (stage B, `bspike-result.md`): a between-turns `session/resume` with the folder added to
`additionalDirectories` works in process (about 0.05 s), and plain writes then run inside the
sandbox with no card. The grant answers the card allow-once and is applied when the turn ends,
never while a card is open (that raises a duplicate card). A re-resume resets codex-acp's
in-session mode and token counters, so Clauseo's mode is applied again after it. codex-acp marks
an added folder a trusted project, so a `.codex/` inside it would load (config, hooks, rules): a
Codex `folder_save` grant is refused for a folder that holds `.codex`, checked again at each
launch and re-resume.

Trust probe (stage B, `trustprobe-result.md`): a Codex `folder_save` grant is also refused when
the folder is an ancestor of the matter (`.codex` there loads on a fresh `session/new` or `load`,
or after a crash recovery) or holds `.agents/skills` (loaded at once, with no trust,
process-wide). The check runs at grant, at each launch and re-resume, and per prompt, in one
function shared with PR 2's `.codex` check (today's `refuse_codex_folder` checks only
`<matter>/.codex`). After every re-resume the session mode is applied again (it reverts to the
launch mode: read-only would become workspace-write), with a test that the next turn has the same
mode and denies plus only the folder. `sandbox_mode` stays out of `home_config()`, pinned by a
test; otherwise Codex writes `[projects.<git root>] trusted` into the home `config.toml`. The
counter reset is harmless for the UI.

Orchestrator decisions for that work: the refusal is a plain sentence (words proposed on the
wording page); a `.codex`, `.agents/skills` or ancestor case that appears later withdraws the
grant. The test after a re-resume fails on any widening: mode, approval policy, writable roots
other than the one granted, or network. If the mode cannot be applied again, the conversation
stops with a sentence and never goes on in the wider mode.

### What still asks every time

| Case | Example | Census |
|---|---|---|
| A command with a shell, interpreter or runner | `swift read.swift Brief.pdf`, `python3 x.py`, `bash -c "…"`, `npx …` | S5:22, S5:32 |
| A chain, pipe, redirection or substitution | `cd sub && ls`, `ls > list.txt`, `cat $(ls)` | |
| A command with an argument outside the matter, both agents | `mv ../Q ../Q2`; Codex's `cp witness-statement.docx "/Users/Shared/…/witness-statement.docx"` | S249:25 |
| A command with a web address (Codex) | `curl -L -o judgment.pdf https://…` (refused unasked, `shell_network.rs`) | S3:48, S22:63 |
| Network approvals for Codex | any host (Codex sites: `docs/notes/codex-network-sites.md`) | |
| A Codex command with a backslash | `type C:\m\a.txt` (Codex on Windows) | |
| An ordinary command in a new folder | `mdls a.pdf` in `Drafts` after it was allowed in the matter folder | S5:33 |
| `git` | `git status` (hooks and config can run programs) | |
| A file outside the matter | an edit of `../B/notes.txt` | row 10 |
| A Codex write outside the matter (its only cards in the live record) | until the folder grant (`folder_save`, below) lands | |
| A Codex MCP tool of a lawyer's server not trusted | asked once per conversation ("For this conversation") | |
| `recharge` | case law's credit purchase | |
| No secret store | Linux, or a refused Keychain prompt | row 21 |
| A command longer than 8 KiB, cut, or holding `[withheld]` | | |

### The real cards (live record, `/tmp/proposals/result.md` §3 and §5)

Codex (gpt-6-luna) asks only for writes outside the matter, and its proposed "prefix" is always
the whole command (it never sends `prefix_rule`). Each card's request:

```json
{"toolCall": {"kind": "execute", "title": "<command>",
  "rawInput": {"command": "<command>", "cwd": "/Users/Shared/proposals-live/matters/codex3"}},
 "options": [{"optionId": "allow_once", "kind": "allow_once"},
  {"optionId": "accept_execpolicy_amendment", "kind": "allow_always"},
  {"optionId": "cancel", "kind": "reject_once"}]}
```

| Command | Under this record |
|---|---|
| `cp witness-statement.docx "/Users/Shared/proposals-live/outbox/codex3/witness-statement.docx"` | One plain command, but an argument outside the matter: no "Always" (Allow once / Don't allow). `accept_execpolicy_amendment` is dropped. |
| `python3 -c "from pathlib import Path; Path('…/covering-note.txt').write_text('…')"` | An interpreter: no "Always". |
| `cupsfilter "…/engagement-letter.txt" > "…/engagement-letter.pdf"` | A redirection: no "Always". |

Claude in `auto` raised no card in any task of the record.

### Deviations from the brief and the first draft

- **Codex commands with an argument outside the matter stay ineligible** (proposed, then vetoed
  by the orchestrator). An exact command replayed unsandboxed is a standing hole: the source
  name inside the matter can later be swapped, by a planted document, for a link to another
  matter's file, and the remembered `cp` would copy it out with no card and no sandbox. The
  replacement comes later: "Always" on a card whose only reason is a write outside the matter
  will keep the destination folder (`folder_save`, "Always allow saving to <folder> in this
  matter"), made a writable root of that matter's Codex sandbox, so the write runs inside it.
  `AlwaysKind` is an open enum for it.
- **File edits are matter-wide** (`Edit(*)`), not per file as #1109 kept them; a per-file
  `Edit(//…)` read back is not well formed. `Read(//…)` stays per file.
- **Every agent `allow_always` option is dropped**, not only the listed ids.
- **`recharge` never gives a rule**, so its card offers no "Always".
- **Codex on Windows** reads the shown command as PowerShell; a backslash refuses.
- **Protocol**: `AlwaysKind` has a fifth kind, `read`; `KeptPermission.cwd` carries a kept
  Codex command's folder.
- **Forgetting** a matter deletes its file; bringing the matter back (an import) starts empty.

## Consequences

- Neither agent writes a rule anywhere, so nothing outlives the matter or leaks across matters.
- The parser fails closed. Ordinary harmless commands written with a pipe or a quote it refuses
  (`ls | head`) still ask every time. That is the cost of exact, fail-closed matching.
- Removing an entry takes effect on the actor at once, before the file write (#1109's ledger).
- Codex network sites are a later PR (`docs/notes/codex-network-sites.md`).

## Status

accepted (stage B, 2026-11-16)

## Date

2026-11-16
