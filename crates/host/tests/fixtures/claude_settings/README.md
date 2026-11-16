# Claude Code's settings schema, trimmed

`settings.schema.json` is the part of Claude Code's settings schema that Clauseo's flag settings
(`claudeCode.options.settings`, the `--settings` source) touch, for Claude Code 2.1.293.
`src/agent/loads/settings_schema_tests.rs` checks every settings object
`agent::loads::session_meta` builds against it, with the small checker
in `src/agent/loads/settings_schema_tests/check.rs` (no JSON Schema crate is a dependency).

## The pin

- `@anthropic-ai/claude-agent-sdk` 0.3.293, whose `package.json` says `"claudeCodeVersion":
  "2.1.293"`: the SDK and CLI that claude-agent-acp 0.89.1 runs (the pin named in
  `src/agent/loads.rs`'s module doc and the decision records).
- Source: `sdk.d.ts`, `export declare interface Settings` at lines 6738-9374 ("auto-generated
  from the settings JSON schema", `SettingsSchema` in Claude Code's `src/utils/settings/types.ts`).
  Hooks are at 7035-7223, `env` at 6823-6825, `permissions` at 6855-6885, `disableAllHooks` at
  7252, `disableSkillShellExecution` at 7288, `skipWebFetchPreflight` at 8690, `sandbox` at
  8691-8928.
- The hook event names come from the CLI binary (`claude-agent-sdk-linux-x64/claude`): the d.ts
  types `hooks` as `[k: string]`, but the zod schema is a partial record keyed by the event enum
  (`gQ=f(()=>BNe(j(ah),A(rn())))`, `ah=["PreToolUse",…]`), so an unknown event name fails.
- The package ships no settings JSON schema file (`sdk.mjs`, `sdk.d.ts` and the binary were
  searched); the d.ts is the published form of it.

## How it was trimmed

- A d.ts object type with `[k: string]: unknown` is open (`additionalProperties: true`): the
  top level, `permissions` and `sandbox`. One without it is closed (`false`): a matcher group,
  each hook type, `sandbox.network`, `sandbox.filesystem`, `ripgrep`, `tlsTerminate`. A record
  (`env`, `ignoreViolations`, `headers`) is an object whose values all have the record's type.
- At run time the CLI's zod objects that the d.ts shows closed are plain `z.object`s: an unknown
  key there is stripped, not refused. Closing them here is stricter on purpose: a key Claude Code
  would strip is one it ignores.
- Of the open objects, only keys Clauseo sends, or might sensibly send next, are listed; any
  other passes, as in the CLI. `sandbox.credentials` and the zod's internal command hook fields
  (`rewakeMessage`, `rewakeSummary`, `cloud`, which the d.ts leaves out) are not listed.
- Hook `timeout` is `E().positive()` in the binary: a number above 0 (`exclusiveMinimum: 0`).
- `defaultMode`'s alias `manual` (accepted by the zod's preprocess) is left out: the d.ts enum
  is what Clauseo writes to.

## Making it again for a new pin

1. Install the new pin's SDK (`npm pack @anthropic-ai/claude-agent-sdk@<version>`, or the
   `node_modules` of a claude-agent-acp install) and read `package.json`'s `version` and
   `claudeCodeVersion`.
2. Find the Settings interface: `grep -n "export declare interface Settings" sdk.d.ts`. For each
   key the schema lists, compare its d.ts type and whether its object has `[k: string]: unknown`.
3. Read the hook event list from the binary:
   `grep -a -o '[,; ]ah=\[[^]]*\]' claude` (the minified name may change; search for
   `"PreToolUse","PostToolUse"` otherwise), and the hook `timeout` rule near
   `Timeout in seconds for this specific command`.
4. Update `settings.schema.json` (its `$comment` names the version and lines) and this file, then
   run `cargo test -p clauseo-host settings_schema`.
