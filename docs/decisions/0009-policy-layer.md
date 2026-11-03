# 0009: The policy layer

## Context

A firm's IT pushes settings to every lawyer's machine and expects them to hold: the user must not
be able to change or hide them. Intune and ADMX write flat named registry values, an MDM pushes a
configuration profile on macOS, and some firms copy a file. No M1 setting is a policy key yet, but
the layer is read from the first start (seam 6), so firms' IT can write against it now and later
keys need no new mechanism. `clauseo-settings` reads it, and IT writes against this record.

## Decision

- **Sources, strongest first.** A key the first source sets is not taken from a later one. Within
  one source, a key set twice (a `policy.json` may repeat one) keeps its last value, as in
  `settings.json`.
  - Windows: the registry key `HKEY_LOCAL_MACHINE\SOFTWARE\Policies\Clauseo\Clauseo`, read
    through the 64-bit view, then `<ProgramData>\Clauseo\policy.json`, where `<ProgramData>` is the
    `FOLDERID_ProgramData` known folder (usually `C:\ProgramData`).
  - macOS: managed preferences for the domain `ai.clauseo.desktop`, then
    `/Library/Application Support/Clauseo/policy.json`.
  - Linux: `/etc/clauseo/policy.json`.
  - Only these count. `HKEY_CURRENT_USER`, roaming app data and user-writable files are never
    policy, and no variable moves a source: `CLAUSEO_DATA_DIR` moves only the data folder and,
    with it, the default matters folder, `<data dir>-matters` beside it (never inside: the
    matters' refused trees hold the whole data folder).
- **Keys** are the dotted keys of `settings.json`: `matters.default_location`, `ui.theme`,
  `ui.font_size`, `ui.density`, `ui.language`, `ui.lite`, `ui.motion` (the last two added by
  0017), `agents.<id>` for one agent's launch spec, and `mcp.servers` for the whole list of MCP
  servers a lawyer can turn on (added with clauseo-ai/clauseo-acp-private#160).
- **Registry format.** One value per dotted key. `REG_DWORD` is a number, or a boolean (0 or 1)
  where the setting is one. `REG_SZ` is a string, or JSON text where the setting is an object
  (`agents.<id>`, `mcp.servers`). `REG_MULTI_SZ` is a list of strings. A value of another type is a warning (and
  locks its key, below). Value names are compared without case, as Windows does: `UI.Theme` is
  `ui.theme`, and `Agents.Claude` is `agents.Claude`, whose id keeps its case.
- **Managed preferences.** A key counts only when `CFPreferencesAppValueIsForced` says it is
  forced; a value the user wrote with `defaults write` is not policy. Keys are read one by one, so
  agents sit under one key, `agents`, a dictionary from agent id to launch spec; an `agents.<id>`
  key is never read on macOS. Property list values become JSON values (dictionary, array, string,
  number, boolean); a date, a data value or a number that is not finite is a warning (and locks its
  key, below).
- **`policy.json`** has the shape of `settings.json`: sections `matters`, `agents`, `mcp` and `ui`, each
  an object, in JSON with comments and trailing commas.
- **Merge rule.** Defaults, then the user's `settings.json`, then policy. A policy value replaces
  the user's value for its key, and the key shows as locked. An agent's policy entry replaces the
  user's whole entry for that id (args and env are not merged), and the locked key is
  `agents.<id>`.
- **MCP servers** (Rohan, 2026-11-02). `mcp.servers` maps a server name (1 to 64 letters, digits,
  `_` or `-`) to `{command, args, env, enabled}`, with `command` an absolute path and
  `enabled` `false` unless set. It is one key: a policy value replaces the user's whole list, with
  the firm's `enabled` values, and locks it. An entry with a bad name, a relative `command` or a
  value that does not type is left out with a warning, and the rest of the list loads; an unknown
  field in an entry is a warning and is ignored. Only enabled servers reach an agent's session.
  On macOS it is the managed-preferences key `mcp.servers`, a dictionary.
- **What is left out.** A key this version does not know is a warning and locks nothing, so one
  profile serves machines on mixed versions.
- **An invalid policy value fails closed, key by key** (Rohan, 2026-10-31). A value of the wrong
  type for a known key is a warning, and the key is still locked, at its default: the user's value
  never stands in for a broken policy value. An agent whose `command` is not an absolute path, or
  whose launch spec does not type, is a warning, and `agents.<id>` is still locked with no entry:
  the user's entry for that id is dropped, so that agent does not launch. A stronger source's
  invalid value wins over a weaker source's valid one, as any value does. A malformed section of a
  `policy.json`, or a managed-preferences `agents` value that is not a dictionary, names no key,
  so it is only a warning, except where the section holds a security-relevant key (below). A
  policy `mcp.servers` that is not an object locks the list empty, so no server reaches an agent.
  So does a policy that names MCP servers in a form this version cannot read, as far as each
  source can see it (review of PR #117). `policy.json` and the registry list their own keys, so
  in them an `mcp` section that is not an object (`"off"`, `[]`, `null`), and any unknown key
  whose name holds `mcp` in any case, at the top level or inside the section (`"mcp.servers"` at
  the top of `policy.json`, `"MCP"`, `"mcpServers"`, `mcp_servers`, `mcp.Servers`,
  `mcp.servers.firm` written as one key), lock the list empty, even beside a valid `mcp` section, with a warning naming the shape that sets it.
  macOS managed preferences are read key by key by exact name, so a profile must use
  `mcp.servers` exactly; any other name there is never seen. An entry left out inside a valid
  list, an unrelated unknown key, and an empty `"mcp": {}` (as for any section) lock nothing, and
  neither does a key under `agents.` whose agent id holds `mcp` (`agents.mcp-proxy`): it is an
  agent's key. In the user's `settings.json` all of this is only a warning. This replaces the
  first draft's rule, under which an invalid value was
  left out and its key not locked.
- **Applied sources.** `PolicyStatus` lists a source as applied only when it set at least one
  valid value; a source whose every value was refused still locks the keys it named.
- **A source that cannot be read** (a `policy.json` that does not parse or cannot be opened, a
  registry read that fails) is dropped whole. Start-up goes on with the other sources, and the
  error is listed in `PolicyStatus` for `host` to log. **A security-relevant key fails closed on an
  unreadable source too** (Rohan, 2026-11-02): what the source held for it is unknown, so the key
  is locked at its default, with a warning, unless a stronger source already set it. Today that is
  only `mcp.servers` (`SECURITY_KEYS` in `crates/settings/src/leaf.rs`), so a broken firm file
  turns every MCP server off. Other keys keep the user's value.
- **`ARCHITECTURE.md`.** The `settings` row becomes `exists`, and its "May depend on" cell lists
  the normal dependencies the crate uses: `serde, serde_json, serde_json_lenient, schemars;
  windows (Windows only); objc2-core-foundation (macOS only)`.

## Consequences

- A new policy key is a new settings key: add it in each place the checklist at the top of
  `crates/settings/src/leaf.rs` names (the schema, the key table and the rest), and the registry,
  managed preferences and `policy.json` read it.
  A boolean or string-list key is read from `REG_DWORD` and `REG_MULTI_SZ` as above.
- Managed preferences cannot list unknown keys, since they are read key by key; an unknown key
  there is not reported.
- The policy layer is read once at start-up. A change takes effect on the next start.

## Status

proposed

## Date

2026-10-23
