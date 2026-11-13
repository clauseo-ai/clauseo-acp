# claude-rule-probe

Asks a real Claude Code binary which commands its Bash deny rules refuse. It drives the binary
offline: a local stand-in for the Messages API (`fakeapi.py`) answers with one tool use
carrying each command, and records the result Claude Code sends back. It needs no login and no
network, and nothing in CI runs it. It exists to check `data_fence/shell_deny.rs`'s rule
escaping against the pinned Claude Code (decision record 0030, #1035).

```sh
# The pinned binary: @anthropic-ai/claude-agent-sdk-linux-x64 (the version in the agents'
# pins), unpacked from npm's cache or `npm pack`, at package/claude.
python3 scripts/claude-rule-probe/probe.py <path to package/claude> scripts/claude-rule-probe/cases.json
```

`cases.json` holds the hand-checked readings: a `\` before a letter, `\\`, `\*`, `\\\*`, and the
first #1035 build's blanket rule. Each case says whether it must be denied, and the script exits
1 on a disagreement. Only the Bash tool exists off Windows: a PowerShell case is reported as
"no tool". PowerShell rules go through the same parser and matcher, in any case.

The binary sometimes matches a command as its own shell parse reads it, not only as written.
An unquoted `\` after a variable (`type $HOME/…\agent-state\…`) is dropped there, as Bash
drops it, so a rule with that `\` may not match. That is harmless: Bash then runs a name that
does not exist (`…desktopagent-state…`). The port in `shell_deny_tests.rs` reads the command
as written only, so it denies more. A case whose `denied` is `null` is reported, not checked.

The host's tests can write a launch-sized cases file: the Bash rules of a laptop-like data
folder, the Windows VM's commands, and a sample of own and protected spellings. Run
`CLAUSEO_RULE_PROBE_OUT=<file> cargo test -p clauseo-host --lib laptop_like`, then give that
file to `probe.py`.
