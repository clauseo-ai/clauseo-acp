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
and a sibling, a chained part or another command is refused or denied. To write a launch-sized
pair of files, run
`CLAUSEO_OWN_ALLOW_PROBE_OUT=<file> cargo test -p clauseo-host --lib probe_cases_for_the_pinned_binary`.
It writes `<file>`, where the server flags every call, and `<file>.local`, where the server
sends no verdict, so a command no rule allows asks the local classifier. Off Windows a `C:\…`
path is a relative one, inside the working folder. Git Bash's `/c/…` is absolute there, so
those cases are left out. An unquoted trailing `\` goes to a classifier: Claude Code reads its
own parse, not the text.
