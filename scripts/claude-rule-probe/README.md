# claude-rule-probe

Asks a real Claude Code binary which permission rules it accepts and which commands its Bash
deny rules refuse. It drives the binary offline: a local stand-in for the Messages API
(`fakeapi.py`) answers with one tool use carrying each command, and records the result Claude
Code sends back. It needs no login and no network, and nothing in CI runs it. It exists to check
`data_fence/shell_deny.rs`'s rules against the pinned Claude Code (decision record 0030, #1035).

```sh
# The pinned binary: @anthropic-ai/claude-agent-sdk-linux-x64 (the version in the agents'
# pins), unpacked from npm's cache or `npm pack`, at package/claude.
python3 scripts/claude-rule-probe/probe.py <path to package/claude> scripts/claude-rule-probe/cases.json
```

It does two checks:

- **Rules.** Claude Code loads the settings as its `--settings` file and lists the rules it
  kept (the SDK's `list_permission_rules` control request). Every rule must be among them except
  those in `expect_skipped`. Rules are compared as Claude Code reads them, because it lists them
  in its own spelling. `claude doctor` then reads the same settings as a project's
  `.claude/settings.json`. It must report exactly the expected
  `Invalid permission rule "…" was skipped` warnings.
- **Commands.** Each case's command goes to the Bash tool, and the result must match its
  `denied`. A case whose `denied` is `null` is reported, not checked. Only the Bash tool exists
  off Windows, so a PowerShell case is reported as "no tool". PowerShell rules go through the
  same loader, parser and matcher, in any case.

`cases.json` holds the hand-checked readings:

- a `\` before a letter;
- `\\`, `\*` and `\\\*`;
- the first #1035 build's blanket rule;
- a rule whose content ends in one backslash, which escapes its own closing `)` and is skipped;
- the same rule with two backslashes, which is kept.

Claude Code matches a Bash command as its own shell parse reads it, not only as written. The
parse drops an unquoted trailing `\`, and an unquoted `\` after a variable
(`type $HOME/…\agent-state\…`), as Bash drops them. So a Bash rule that needs either may not
match. That is harmless: Bash then runs a name without that `\`. The port in
`shell_deny_tests.rs` reads the command as written only, so it denies more. A rule ending in a
backslash is there for PowerShell, which keeps it.

The host's tests can write a launch-sized cases file. It holds the whole permission settings of
a laptop-like data folder (Read and Edit, allow, and the shell rules of both shells), the Windows
VM's commands, commands ending in a separator, and a sample of own and protected spellings.
Run `CLAUSEO_RULE_PROBE_OUT=<file> cargo test -p clauseo-host --lib laptop_like`, then give that
file to `probe.py`.

They also write one probe per rule shape, so that a rule which loads but never matches shows
up as a probe the binary lets through. The shapes cover how the data folder is written, what
the rule names, and its separators and boundary characters. Run
`CLAUSEO_RULE_SHAPES_OUT=<dir> cargo test -p clauseo-host --lib -- --ignored the_full_rule_shape_sweep`
(the full sweep, ignored in normal runs; the normal test probes one rule a shape through the
port only). It writes
`default.json`, `configured.json` and `configured-powershell.json`; the last holds the
PowerShell-only rules as Bash rules of the same content. Every case must come back denied.
Thousands of probes take a while, so split a file into parts and run them side by side.
`PROBE_SKIP_RULES=1` skips the rules check in all parts but one.

Claude Code puts its own `HOME` in place of a Bash `$HOME` that the shell would expand
(unquoted or in double quotes) before it matches. So a rule that writes `$HOME/…` literally
matches only `'$HOME/…'`, and the launch also writes the home folder's value (`C:\Users\…/…`).
For the host's generated cases, set `PROBE_HOME='C:\Users\clauseo-lawyer'`, the laptop's home.

## Auto mode and the classifiers

A case may set `"mode": "auto"`. Auto mode needs no gate offline. In auto, Claude Code 2.1.293
decides what no rule settles in one of two ways:

- **The local classifier.** A command that asks goes to Claude Code's own classifier, a
  separate Messages request. It has no tools, and its system prompt starts "You are a security
  monitor for autonomous AI coding agents." `fakeapi.py` logs every request (`requests.jsonl` in
  the probe's folder), spots this one, and answers "allow". With `CLASSIFIER_VERDICT=block` it
  answers "block". A case's `"classifier": true|false` checks whether this request was sent.
- **The server-side classifier.** A command allowed only because it is read-only (`ls`, `cat`
  in a working folder), or because the sandbox wraps it, goes to the server instead
  (`nTr`, `Hln`). Its verdict comes back with the reply that carried the tool use. A file's
  `"server_verdict": "flagged"` (or `"not_flagged"`) makes the fake API send that verdict
  (`SERVER_VERDICT`). Without one, Claude Code takes the server as not classifying for the rest
  of the session.

A command an allow rule matches skips both. A case's `"ran": true|false` checks the verdict:
`DENIED` (a deny rule), `REFUSED` (a path check, a safety check, a classifier, or a prompt that
`-p` refuses), or `ran`. `cases.json` ends with the own folders' allow: what a rule allows runs,
and a sibling, a chained part or another command is refused or denied. So is a path outside the
working folders beside the own one (`/etc`, absolute off Windows as `C:\…` is on it), since
`ls`, `cat` and `head` are in Claude Code's table of path-checked commands. Its last case shows
why only those may have a rule: `dir` is not in the table, so a `dir` rule lets `dir /etc <own>`
through. To write a launch-sized
pair of files, run
`CLAUSEO_OWN_ALLOW_PROBE_OUT=<file> cargo test -p clauseo-host --lib probe_cases_for_the_pinned_binary`.
It writes `<file>`, where the server flags every call, and `<file>.local`, where the server
sends no verdict, so a command no rule allows asks the local classifier. Off Windows a `C:\…`
path is a relative one, inside the working folder. Git Bash's `/c/…` is absolute there, so
those cases are left out. An unquoted trailing `\` goes to a classifier: Claude Code reads its
own parse, not the text.

## The matter's reads

A file may give `"cwd"` (the folder Claude Code works in, default a temp folder) and `"env"`
(variables for the binary). With `"CLAUDE_CODE_USE_POWERSHELL_TOOL": "1"` and `pwsh` on the
`PATH` (PowerShell 7's Linux tarball works unpacked anywhere), 2.1.293 offers its real
PowerShell tool off Windows, so PowerShell cases run instead of "no tool". `PROBE_VERBOSE=1`
also prints each case's request count, its local classifier requests and the beta headers.

`CLAUSEO_MAC_MATTER_PROBE_OUT=<dir> cargo test -p clauseo-host --lib mac_probe_cases` makes a
laptop-like data folder and a matter under `<dir>/root` (real paths, a fake `stetdoc` first on
the files' `PATH` and a planted one in the matter) and writes five files with the launch's
`Read`/`Edit` denies and allow rules, the profile's stetdoc rules, the matter as the working
folder and the own folders in `additionalDirectories` (decision record 0030, "The matter's
reads"). The files' `PATH` is the one the test ran with, so put `pwsh` on it first.

- `flagged.json`: the server flags every call. Matter reads run with no classifier request;
  outside paths, `..` and links out, siblings, chained commands, writes, network and the
  left-out commands are refused; protected places are denied.
- `local.json`: no server verdict, so what no rule allows asks the local classifier (or, when
  Claude Code counts it read-only, runs unjudged: reported only).
- `baseline.json`: flagged, without the matter rules: the matter reads are refused.
- `grep.json`: flagged, with a `grep` rule of the matter's shape. `grep --context x <outside>
  <matter>` runs unchecked, which is why `grep` has no rule.
- `powershell.json`: flagged, PowerShell 7 with the matter's PowerShell rules for this path and
  the Windows stetdoc rules. A mixed line (`Get-ChildItem <matter>; stetdoc --help`) runs; a
  line read-only as a whole is still refused (the server's); `./stetdoc` matches no rule.
- `glob-h1.json`, `glob-h2.json`: the security review's glob lines (#1065, H1: the home folder
  as the matter, `<data>` inside it; H2: a link in the matter to `<data>`, and a glob past a
  `Read` deny), with the launch's real rules: the matter gets none, so each is refused. The
  `-before` files force the matter rules as f7adde050 wrote them (reported only).

```sh
ulimit -f 1048576  # a case that writes where it reads must not fill the disk
export DOTNET_EnableWriteXorExecute=0  # else pwsh's .NET dies under that cap
python3 scripts/claude-rule-probe/probe.py <package/claude> <dir>/flagged.json
```
