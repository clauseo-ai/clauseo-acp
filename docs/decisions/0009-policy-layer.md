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
    policy, and no variable moves a source: `CLAUSEO_DATA_DIR` moves only the data folder.
- **Keys** are the dotted keys of `settings.json`: `matters.default_location`, `ui.theme`,
  `ui.font_size`, `ui.density`, `ui.language`, and `agents.<id>` for one agent's launch spec.
- **Registry format.** One value per dotted key. `REG_DWORD` is a number, or a boolean (0 or 1)
  where the setting is one. `REG_SZ` is a string, or JSON text where the setting is an object
  (`agents.<id>`). `REG_MULTI_SZ` is a list of strings. A value of another type is a warning (and
  locks its key, below). Value names are compared without case, as Windows does: `UI.Theme` is
  `ui.theme`, and `Agents.Claude` is `agents.Claude`, whose id keeps its case.
- **Managed preferences.** A key counts only when `CFPreferencesAppValueIsForced` says it is
  forced; a value the user wrote with `defaults write` is not policy. Keys are read one by one, so
  agents sit under one key, `agents`, a dictionary from agent id to launch spec; an `agents.<id>`
  key is never read on macOS. Property list values become JSON values (dictionary, array, string,
  number, boolean); a date, a data value or a number that is not finite is a warning (and locks its
  key, below).
- **`policy.json`** has the shape of `settings.json`: sections `matters`, `agents` and `ui`, each
  an object, in JSON with comments and trailing commas.
- **Merge rule.** Defaults, then the user's `settings.json`, then policy. A policy value replaces
  the user's value for its key, and the key shows as locked. An agent's policy entry replaces the
  user's whole entry for that id (args and env are not merged), and the locked key is
  `agents.<id>`.
- **What is left out.** A key this version does not know is a warning and locks nothing, so one
  profile serves machines on mixed versions.
- **An invalid policy value fails closed, key by key** (Rohan, 2026-10-31). A value of the wrong
  type for a known key is a warning, and the key is still locked, at its default: the user's value
  never stands in for a broken policy value. An agent whose `command` is not an absolute path, or
  whose launch spec does not type, is a warning, and `agents.<id>` is still locked with no entry:
  the user's entry for that id is dropped, so that agent does not launch. A stronger source's
  invalid value wins over a weaker source's valid one, as any value does. A malformed section of a
  `policy.json`, or a managed-preferences `agents` value that is not a dictionary, names no key,
  so it is only a warning. This replaces the first draft's rule, under which an invalid value was
  left out and its key not locked.
- **Applied sources.** `PolicyStatus` lists a source as applied only when it set at least one
  valid value; a source whose every value was refused still locks the keys it named.
- **A source that cannot be read** (a `policy.json` that does not parse or cannot be opened, a
  registry read that fails) is dropped whole. Start-up goes on with the other sources, and the
  error is listed in `PolicyStatus` for `host` to log. No M1 key is security-relevant; M2 revisits
  this when firm keys arrive, where failing closed may be right.
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
