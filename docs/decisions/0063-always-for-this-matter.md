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
byte: `rawInput.command` as the agent sent it (Claude's `Bash`/`PowerShell` input; Codex's
`stripShellPrefix(shlex_join(argv))`). Eligibility is an **allowlist**
(`always/table.rs`, read by `always/plain.rs`). A command is kept only when all of these hold:

- It parses as **one plain command**: words of ASCII letters, digits and `_ - . / : , + = @ %`
  (PowerShell: `_ - . / : +` only), or `'…'` and `"…"` holding those and spaces; no other
  letter (no look-alike names). In PowerShell a quoted word never starts with `-` (PowerShell
  reads it as a file, never a switch), and a bare word that starts with a digit, `+` or `.` and
  a digit must be a plain number of at most nine digits with no leading zero (PowerShell reads
  `1kb` as 1024, `0x10` as 16, `007` as 7). Anything else
  fails closed: `; & | < > ( ) $ ` \ ! ^ * ? [ ] { } ~ #`, any quote mark inside a quote (PowerShell
  ends a string at a typographic quote too), a line break, a control or invisible character.
- The program is a **bare name on the list**, never a path (a program in the matter is a file the
  agent can write). The list, from the 2026-11-16 card census and the plain commands beside it:
  `ls cat head tail wc file grep mdls`, `stetdoc view|inspect|check`, and the cmdlets
  `Get-Content Get-ChildItem Get-Item Test-Path Select-String`. **Every program on it only
  reads.** Nothing that writes (`cp`, `mv`, `mkdir`, `Copy-Item`, `Move-Item`), runs code,
  reaches the network, deletes, links or runs another program is on it, so no runner,
  interpreter, preprocessor flag or file-reading config can come in by not being named. A write
  lands through a link the agent places after the check (in a destination folder, or in the race
  below), and a recursive copy or move could put `AGENTS.md`, `.codex` or `.agents` at any
  depth.
- Every option is one the program's entry lists, with a value of the kind it says (a number, a
  text the program never opens, or a path). One-letter flags may be joined (`-la`) only when none
  takes a value; `--name=value` only for a listed long option; PowerShell takes no
  `-Param:value` form and no joined flags. In a POSIX command no option comes after the first
  operand: BSD `cat`, `head`, `ls` and `wc` stop reading options there, so `cat notes.txt -n`
  reads a file named `-n`. No recursive search or listing (`grep -r`, `ls -R`), so no kept
  command reads `.clauseo` or a `.claude` folder. With `-Pattern` given by name,
  Select-String's operands are all paths.
- Every path word is a plain path: no leading `-` or `=` (zsh's `=prog`), no `=` or `~`, no
  `\`, no `:` but a drive letter's (`HKCU:`, `Env:`, `FileSystem::` refuse), no `..` name.
  None names an absolute place outside the matter.
- Codex's card: `toolCall.kind` `execute` and `rawInput` holding only `command` and `cwd`. A
  card that also asks for a site (`url`) or more sandbox permissions (`additionalPermissions`)
  gives no rule, so a kept rule never grants more than the plain run it was kept for.
- Codex's own wrapper is read through only at the paths Codex launches: `/bin/zsh`,
  `/bin/bash` or `/bin/sh` with `-c` or `-lc` and one script; on Windows
  `C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe [-NoProfile|-NoLogo|-NonInteractive]
  -Command <script>`. A shell by a bare name, a relative path or a path in the matter is a
  program off the list. Claude's Bash string is the script; a `bash -c` the model writes is off
  the list too.

**What a command reaches, by real path** (`always/reach.rs`). The matter thread checks it when
the card is about to offer "Always" and again before every answer the host gives from the rule
(`MatterJob::HeldOpen`, `Reach::check`). When it fails, the lawyer is asked:

- The working folder and every path word, resolved through every link and junction, are the
  matter folder or inside it, under none of the names in the next paragraph. A file swapped for
  a link to another matter after the rule was kept is caught at the next use.
- The program is the first file of its name on the agent's own `PATH` (as its launch set it), and
  by real path in a folder its entry names: `/bin` or `/usr/bin` (`mdls` `/usr/bin` only; Git
  for Windows' `usr/bin` for Claude's Bash on Windows), or for `stetdoc` stetdoc's own folder
  when Clauseo puts it first on that `PATH` (never the browser's tools or the profile hook's
  folders). Never in the matter; a relative `PATH` entry before it fails. A cmdlet is part of
  PowerShell and is matched by name.
- For either agent, no entry of the working folder bears the program's name: codex-acp shows a
  run of `bash <name>` as `<name>`, and a `.` a user's shell files put on the search path would
  run that file.

Claude's working folder is always the matter folder: Clauseo sets
`CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1` (`agent/loads.rs`), and Claude Code then starts
every Bash call there (its binary resets the shell's cwd after each call). So a Claude rule needs
no folder, and its card's `{folder}` is "this matter's folder". A firm's Claude Code policy can
turn that variable off; the start is then unknown (`sent::ShellStart`), and no Claude command is
offered "Always" or answered from a kept rule. A Claude shell call that asks to run outside its
sandbox (`dangerouslyDisableSandbox`) keeps nothing either: a kept rule covers the plain run.

**A helper's requests.** A Codex `multi_agent` helper asks on the parent's session with a tool
call the host never saw announced. A kept rule answers the helper's identical command (same
text, same folder, same reach checks) as it answers the parent's. Its card never offers "Always":
the card shows a generic line, not the command, and the lawyer would keep a rule they never saw.
The same holds for any request whose tool call the host did not log. The host's answer names the
kept rule in `PermissionResolved.always` (its exact command and folder), so the thread shows what
was allowed though the helper's call was never logged.

**File edits.** One `Edit(*)` per matter covers every file whose real path is inside the
matter folder. The matter thread resolves the path the same way the held-open check does:
links and junctions followed, `..` resolved, case folded on Windows and macOS, trailing dots and
spaces dropped on Windows. A network path in its UNC form is compared after the same
resolution. The names agents and Clauseo read their own settings, skills and instructions
from are never inside, at any depth and in any case: `.clauseo`, `.claude`, `.codex`, `.agents`,
`.mcp.json`, `AGENTS.md`, `AGENTS.override.md`, `CLAUDE.md`, `CLAUDE.local.md` (`OWN_NAMES`). A
path that fails to resolve asks.

**Answering.** A later card matching an entry is answered by the host after the data-fence and
held-open checks, with a **one-time** approval:

- Codex: `allow_once` (`Approved`).
- Claude: `allow-once`.

It is logged as `PermissionRequested` and `PermissionResolved` in one commit
(`by: local_desktop`). The resolution's `always` holds the kept entry's `AlwaysOffer`, and
only the host's own answer sets it: the app shows UC-814 when `always` is set, and a lawyer's
"Allow once" on a card that offered "Always" leaves it out. The log line holds the thread id, the agent kind, the entry kind and
nothing of the command (0047).

An entry answers only threads whose matter is its matter (`threads.matter_id`). A thread that
reads another matter's files under an invite still uses only its own matter's entries.

**Options the host sends.** An agent option is dropped on arrival when it would write a rule or
claims a scope it doesn't have:

- Codex: `accept_execpolicy_amendment`, `apply_network_policy_amendment:*`, `allow_for_session`,
  `allow_permissions_turn` (kind `allow_once`, but it grants more permissions for the rest of
  the turn), and MCP `allow_always`.
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
- `clauseo.session:<agent id>` (kind `allow_session`, not the agent's `allow_always`): an
  agent's session-only option (Codex MCP `allow_session`), shown as "For this conversation".
  Answering it sends the agent's own id; the desktop confirms it as it confirms `allow_always`.

A tool that spends the lawyer's money (case law's `recharge`, `always::SPENDS`) offers neither:
its card keeps only the agent's one-time options (`options::once_only`), for both agents,
recognised by Claude's tool name or by the MCP call codex-acp announced for the request.

When the host answers from a kept rule but cannot log the answer, it asks the lawyer instead. When
the matter-thread check cannot be sent, the request reaches the lawyer with no "Always" and no
rule answers it.

Plan-exit options pass unchanged.

**Forced ask (Windows network or unprotected folders, 0030).** The store applies there too. The
host also answers, with no card, the tools of servers it injected itself
(`asks::bundled_tool_to_allow`): case law's `search_law`, `read_document`, `citator`,
`check_citations` and `account`, by exact name, on the reserved `case-law` server Clauseo sent.
`recharge` and any tool that spends money always asks.

**Protocol.** `PermissionRequested.always: Option<AlwaysOffer { kind, shown, cwd, server, tool }>`
tells the card what "Always" would keep: `shown` is the exact command, the site, or a read file's
path relative to the matter folder; `cwd` a Codex command's folder relative to the matter; a tool
has no `shown` but its MCP `server` (none for web search) and its `tool` name there, for UC-819's
"{tool} from {server}". #1109's `HostQuery::KeptPermissions` and
`HostCommand::RemoveKeptPermission` list and remove entries, and `KeptKind` gains `command` and
`file_edit` meanings for the new rules (below).

### Proposed words (provisional, for the wording page)

| Id | Where | Words |
|---|---|---|
| UC-810 | card, command | Always allow this exact command in {folder}. (`{folder}` = cwd, or "this matter's folder") |
| UC-811 | card, file edit | Always allow changes to files in this matter. |
| UC-812 | card, site | Always allow {site} in this matter. |
| UC-814 | thread, host answered | Allowed: always for this matter. |
| UC-816 | empty list | Nothing is always allowed in this matter yet. |
| UC-817 | session option | For this conversation |

The list reuses #1109's "Kept permissions…" dialog and `UBL-26` "Remove". The words are in
`ui/app/copy-approvals/UC-always-for-this-matter.json` (UC-810..812, UC-814, UC-816..819 and
UC-857; UC-818, UC-819 and UC-857 replace #1109's UC-502, UC-503 and UC-507, UC-816 replaces
UC-505). UC-813 ("applies only until Clauseo closes") is gone: no notice shows a failed save,
and a rule the host could not write holds only in the answering agent's memory until that agent
stops. UC-815 ("You said no to this before…") is gone too: nothing refuses a repeat request for
the lawyer, so nothing would show it.

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
| A program off the list | `swift read.swift Brief.pdf`, `python3 x.py`, `curl -sI https://…`, `agent-browser eval …`, `bash -c "…"`, `npx …`, `find`, `rm` | S5:22, S5:32; census 2026-11-16: 26 of 28 shell cards |
| A command that writes | `cp memo.txt Drafts`, `mv Pack Drafts`, `mkdir x`, `Copy-Item a b` | |
| An option off a program's list, or after a file name | `grep -e x a`, `head -n5 a`, `grep -r x .`, `cat notes.txt -n` | |
| A command whose file is now a link out of the matter, or whose program is not where its entry says | `cat notes.txt` after `notes.txt` became a link | |
| A request whose card cannot show the command | a Codex helper's request | |
| A chain, pipe, redirection or substitution | `cd sub && ls`, `ls > list.txt`, `cat $(ls)` | |
| A command with an argument outside the matter, both agents | `cat ../Q/a.txt`; Codex's `cp witness-statement.docx "/Users/Shared/…/witness-statement.docx"` | S249:25 |
| A command with a web address (Codex) | `curl -L -o judgment.pdf https://…` (refused unasked, `shell_network.rs`) | S3:48, S22:63 |
| Network approvals for Codex | any host (Codex sites: `docs/notes/codex-network-sites.md`) | |
| A command with a backslash | `Get-Content .\a.txt`, `type C:\m\a.txt` (PowerShell paths must use `/`) | |
| An ordinary command in a new folder | `mdls a.pdf` in `Drafts` after it was allowed in the matter folder | S5:33 |
| `git` | `git status` (hooks and config can run programs) | |
| A file outside the matter | an edit of `../B/notes.txt` | row 10 |
| A Codex write outside the matter (its only cards in the live record) | until the folder grant (`folder_save`, below) lands | |
| A Codex MCP tool of a lawyer's server not trusted | asked once per conversation ("For this conversation") | |
| `recharge`, and a Codex MCP card the host cannot tie to a tool (it could be `recharge`): only this call | case law's credit purchase | |
| A Claude command when its policy keeps the shell's folder between calls | `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=0` in managed settings | |
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
- **The command check is an allowlist**, not a list of wrappers: a program or option not named
  is asked about each time. Rules kept before it for programs not on the list are dropped on
  read (`well_formed`).
- **Sites are public**: `localhost`, a name with no dot, `.local`, `.internal`, `.lan`,
  `.home.arpa`, and loopback, private, link-local and shared IPv4 addresses are never kept.
- **A malformed key** (not 32 bytes) in the secret store is an error: it is never replaced, and
  nothing is kept or offered.
- **No program that writes is kept** (second security review, N1): `cp`, `mv`, `mkdir`,
  `Copy-Item` and `Move-Item` left the list; rules kept for them are dropped on read.
- **UC-813 is gone**: a rule the host could not write lasts only in the answering agent's memory
  until it stops; other live threads of the matter ask until they relaunch.

### Residuals

- A kept command is checked twice on the matter thread: when its card is about to offer
  "Always", and when the host answers a later request from the rule. A change to the file
  system between that check and the command's run is outside what this layer covers.
- On Windows this layer does not defend the kept-rules file against a change made outside
  Clauseo. The layer that covers the file there is the data fence (decision 0030,
  `agent/data_fence.rs`): Claude Code's deny rules for its file tools and for the shell
  commands it recognises (`DENIED` lists `agent-state/*/claude-allow.json`), and the host's
  auto-deny of a permission request whose paths reach it. It works on what a request or a
  command names, not on what the processes an agent starts open.
- The server names in a firm's managed MCP file are taken as they are: they are not checked for
  a double underscore (`__`), the mark an MCP rule's name uses between server and tool.
- A cmdlet is matched by name: a PowerShell profile that defines an alias or function of the
  same name would run instead (Claude's PowerShell and Codex's `powershell.exe` load profiles
  unless `-NoProfile`).
- `stetdoc` is found only when its real file is named `stetdoc` in its folder.
- MCP rules (`mcp__<server>__<tool>`) cover every argument of the tool, as #1109's did.
- **Protocol**: `AlwaysKind` has a fifth kind, `read`; `KeptPermission.cwd` carries a kept
  Codex command's folder.
- **Forgetting** a matter deletes its file and its set-aside `.bad-<n>` files, ordered after every
  add answered before it (an add still pending is skipped); bringing the matter back (an import)
  starts empty.

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
