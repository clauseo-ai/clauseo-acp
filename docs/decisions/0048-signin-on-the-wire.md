# 0048: Sign-in and installs on the wire

## Context

Stage D gives the lawyer an app that installs its agents and signs them in with no terminal.
`agents` does the work: silent installs (S17, decision record 0033) and sign-in runs, status
checks and login reuse (S18, decision record 0046). Neither crate touches the wire. R20 puts
them in the app: `desktop` composes `agents` beside the `Host`, and the windows drive both. This
record covers what crosses between window and desktop, the new error codes, and how R20's other
pieces must treat it. The brief is `tasks/stage-d/R20-signin-and-installs-in-host.md` (private
repository).

Four facts shape it:
- `protocol` may not depend on `agents` (`ARCHITECTURE.md`). The wire types mirror `agents`'
  states, and `desktop`, which sees both crates, maps one into the other.
- `host` may not depend on `agents` either (ruling 27). Install and sign-in are not host work:
  no thread, matter or log entry is involved.
- Two values a window sends are secrets: the code the lawyer pastes for Claude's paste route, and
  an API key. Neither may reach a log, an event, a settings file or a receipt.
- A window is untrusted, and the two windows (`main`, `settings`) hold different grants
  (decision records 0012 and 0028).

## Decision

**A path of its own.** Setup does not go through `HostCommand` or `HostEvent`. A window sends a
`SetupCommand` through a new Tauri command, `setup`, and desktop answers it from `agents`
(`Done`, or a `HostError`). Desktop sends `SetupEvent`s in a new `setup` arm of its stream
message. `HostCommand` gains no variant and `Bridge::admit` no host arm. The types live in
`crates/protocol/src/setup.rs` and are in `wire_types()`, so the bindings test in `desktop`
exports them to `bindings.ts`.

**`SetupCommand`.** Each variant names its agent (`agent: AgentId`). JSON is the crate's command
form: external tagging, snake case, unknown fields and unknown commands refused.

| command | what desktop does |
|---|---|
| `InstallAgent` | native confirm naming the agent and its size, then the install; another agent's download pauses |
| `RetryAgentInstall { use_system_proxy }` | starts a failed or paused install again, through the OS proxy if asked |
| `StartSignIn { console }` | the plan login (`console`: Claude's Console account); joins a sign-in already running |
| `CancelSignIn` | stops the running sign-in |
| `PasteSignInCode { code: Secret }` | gives the running Claude login the pasted code |
| `UseDeviceCode` | switches Codex to its device code |
| `OpenSignInPage` | opens the current page again ("Didn't open?"); nothing in any other state |
| `SignOut` | signs Clauseo's own login out; `command.invalid` for a reused login |
| `UseFoundLogin` | "Use it": reuses the login `LoginFound` showed; refused in any other state |
| `UseDifferentAccount` | leaves the found or reused login and signs in |
| `StopUsingLogin` | "Stop using it here": stops reuse, never a logout |
| `FindLogin` | looks again for a login the lawyer already has |
| `SetAgentKey { key: Secret }` | stores the key in the OS keychain |
| `ClearAgentKey` | removes it |
| `SetFirmEndpoint { url }` | native confirm, then `plans.<id>.endpoint` set or cleared (`url` missing or `null`) |

A declined confirm answers `transport.unauthorized` and does nothing. **A second `StartSignIn`**
for an agent whose sign-in is already running (`Queued`, `Waiting` or `DeviceCode`), from either
window, joins it: desktop answers `Done` and does not restart the run, and both windows see the
same `SignInChanged` events, the rule `InstallBrowser` follows. "Didn't open?" is
`OpenSignInPage`, not a new `StartSignIn`. S18's engine replaces a running job on a new
`sign_in`, so the join is desktop's job. `SetupCommand::action()` gives the variant as a
`SetupAction` (Rust only), which the allow-lists name.

**`Secret`.** The pasted code and the key are a `Secret`:
- it decodes and never encodes: it has no `Serialize`, so neither does `SetupCommand`, and no
  type holding one can be written through serde to a log, an event, a file or a receipt;
- `Debug` and `Display` print `<secret>`; there is no `Clone`;
- its text is held in `zeroize::Zeroizing<String>` and zeroed when dropped; `expose()` lends it
  and `into_zeroizing()` hands it on (to `agents`' `PasteCode`, or the keychain);
- it holds 1 to `MAX_SECRET_BYTES` (4096) bytes, and a decode error never quotes the text;
- TypeScript sees a plain `string`.

`protocol` gains `zeroize` (already a workspace dependency, used by `agents`) for it. The JSON a
window sent still passes through Tauri's buffers, which Clauseo does not zero.

**`SetupEvent`.** Three payloads, in the crate's event form (external tagging, snake case,
`None` fields left out):
- `SetupInfo { keys_available }`: whether the OS keychain can hold a key (false where Linux has
  no Secret Service; the key screens hide). A new subscription gets it first, then every agent's
  install and sign-in state, before any newer event.
- `AgentInstallProgress { agent, state: InstallProgress }`, S17's `InstallState`:
  `Missing { download_bytes }`, `Waiting { code, hosts, log }` (offline; it retries by itself),
  `Downloading { received, total }`, `Installing`, `Ready { version }`,
  `Failed { code, need_mb, free_mb, hosts, log }`, `Paused`, `Managed`. A waiting or failed
  install carries the snapshot's install hosts for "Ask your IT team" and its `InstallLog` for
  "Details". No end estimate (`ready_by` is dropped). The log is bounded: at most
  `MAX_INSTALL_LOG_LINES` (64) lines of `MAX_INSTALL_LOG_LINE_BYTES` (400) bytes, so at most
  about 26 KB; `InstallLog::new` keeps the two heading lines and the end, and notes how many
  lines it left out, and decoding refuses more. Sizes are `Count`s.
- `SignInChanged { agent, state: SignIn }`, S18's `SigninState` with how the agent is signed in:
  `SignedOut { found }`, `Queued`, `Waiting { since, nudge }`, `DeviceCode { url, code,
  expires_at }`, `LoginFound { from, email }`, `SignedIn { email, plan, method, source, at }`,
  `Failed { code, at }`. `found` is a `FoundLogin` (`claude_settings`,
  `claude_windows_sandbox`, `codex_keyring`, `reused_failed`, `reused_signed_out`,
  `reused_changed`, `codex_link_split`); `from` is a `LoginFrom` (`claude_code`, `codex`);
  `method` a `SignInMethod` (`plan`, `api_key`, `firm`); `source` a `SignInSource` (`reused`,
  the lawyer's own CLI login, or `own`, Clauseo's). `plan` is a `PlanLabel` in words, from
  `limits`' plan words; desktop turns S18's raw plan string into it. Times are `Timestamp`s. A
  device code is shown to the lawyer, so it is a plain string, not a `Secret`.

Install and sign-in failures carry an `ErrorCode`, never `agents`' own kinds. These enums are
closed: setup events are never logged, and window and desktop ship together.

**New error codes.** Eleven, before `unknown`, each with its row in 0003 and in
`docs/ui-states.md` section 9. An unknown code still decodes to `unknown`.

| code | retryable | from |
|---|---|---|
| `agent.install_offline` | yes | S17 `FailureKind::Offline` (the `Waiting` state) |
| `agent.install_blocked` | no | S17 `FailureKind::Blocked` |
| `agent.install_disk_full` | yes (once space is freed) | S17 `FailureKind::DiskFull` |
| `agent.install_failed` | yes | S17 `FailureKind::Failed` |
| `signin.cancelled` | no | S18 `SigninCode::Cancelled` |
| `signin.link_expired` | yes | S18 `SigninCode::LinkExpired` |
| `signin.no_plan` | no | S18 `SigninCode::NoPlan` |
| `signin.device_code_off` | no | S18 `SigninCode::DeviceCodeOff` |
| `signin.port_busy` | yes | S18 `SigninCode::PortBusy` |
| `signin.failed` | yes | S18 `SigninCode::Failed` |
| `signin.policy_unreadable` | no | desktop: policy unreadable with the agent's `firm-policy` marker |

A turn that needs a sign-in keeps the existing `agent.auth_required`; there is no setup twin of
it. `SignIn::Failed` may carry it.

**`SettingsView.plans`.** `SettingsView` gains `plans`, each agent's firm plan by agent id,
left out when empty: a `PlanView` of `endpoint` (an `https` URL), `provider` (`PlanProvider`:
`anthropic`, `bedrock`, `vertex`) and `env_names`, the names of the extra variables. It holds
no variable's value and no key. Each name is an `EnvName`, 1 to 255 characters of
`[A-Za-z0-9_]`, whose constructor and decoder refuse anything else, so `KEY=value` text cannot
ride in one; that the host maps only names into the view, never values, is its own rule and its
tests'. On the wire an `EnvName` is a plain string. The `endpoint` is a URL, and the settings
crate refuses one with a user name, password, query or fragment. Firm secrets stay with IT's own
tools (an AWS profile, Google's application default credentials).

**No `RefreshWhy::LoginChanged`.** "Read the limits now, past the back-off" after a login change
is a Rust-only `HostHandle` call that desktop makes. A wire arm would let a window skip the
back-off.

**Allow-lists.** `setup` is granted to both windows, each with its own list, enforced in
`crates/desktop/src/bridge/admit.rs` (P5) with a refusal test each way:

| window | actions |
|---|---|
| `main` and `settings` | `InstallAgent`, `RetryAgentInstall`, `StartSignIn`, `CancelSignIn`, `PasteSignInCode`, `UseDeviceCode`, `OpenSignInPage`, `UseFoundLogin`, `UseDifferentAccount` |
| `settings` only (Plans) | `SignOut`, `SetAgentKey`, `ClearAgentKey`, `SetFirmEndpoint`, `FindLogin`, `StopUsingLogin` |
| `main` only | none |

A refused action answers `transport.unauthorized`.

**The orchestrator's rulings for R20**, as they bear on this design:
1. *The leave order.* Leaving a reused Codex login (sign-out, an account change,
   `UseDifferentAccount`, `StopUsingLogin`, a key or endpoint set on it, a split link) runs:
   stop the agents, then S18's `stop_using_login` (no-reuse and unlink in one job), then
   `remove_login_links` for the matters, then the new login and the relaunch. No Codex process
   runs while the link changes, and S18 stays as it is. Done-when 1's "no-reuse, stop, unlink"
   order is amended to this.
2. *`LoginCheck`.* `host` runs the split check before a Codex session, the guard before
   `authenticate` and the sign-in-again test through a `LoginCheck` hook in `HostConfig`, which
   desktop fills from `agents` (`codex_link_holds`, `codex_login_shared`, `is_sign_in_again`).
   No file-id code is copied into `host` (ruling 27).
3. *`clear_env`.* Desktop's host launcher moves to `SpawnOptions::clear_env`, like the
   sign-in launcher it gives `agents`: the spec's map already carries the environment a child
   should have, so a variable `spec.rs` drops is really gone (PR "desktop, host: the host
   launcher clears the environment"). An agent launch drops every inherited `SSH_*`,
   `SSH_AUTH_SOCK` and `SSH_ASKPASS` included, on purpose: an agent has no business using the
   lawyer's ssh-agent keys, and they are a way out for data. Checkpoints use gix in-process, so
   agents need no remote git. `limits`' plan readers drop the same (`GIT_*`, `SSH_*`,
   `NO_BROWSER`, `CLAUDE_CODE_REMOTE` and Codex's `DROPPED_VARS`). Desktop gives the `agents`
   worker two launchers: sign-in runs use the `clear_env` one (0046); install runs (npm, Node,
   the system-proxy check) build specs with partial environments and use one that does not
   clear, or Windows children would lose `SystemRoot` and `PATH`.
4. *`LoginChanged`* is a Rust-only `HostHandle` call (above).
5. *`SigninNote`* is logged through `tracing::info!` for now: the agent id and the note's code
   (`claude_reuse_fallback`, `codex_link_failed`) only, never an email or a path, under the
   host log's rules (decision record 0047).
6. *Large install failures.* A `SetupEvent` larger than the channel's message limit goes by the
   event-fragment path desktop already has, not by cutting the log further.

**Logs.** Decision record 0047 has the host log ids, codes and categories only, to a local file.
No setup payload reaches `tracing`: no key, pasted code, email, URL, device code, endpoint or
install log line. Desktop logs a setup action by its `SetupAction`, the agent id and, on failure,
the code.

**Settings and limits keys.** Each key gets its safe value, the value in force when a policy
source cannot be read:

| key | what it is | safe value |
|---|---|---|
| `plans.<id>` | one agent's firm plan, one object (like `agents.<id>`), `<id>` an agent id (`[a-z0-9-]`); a policy that sets it locks the whole entry | no plan: the lawyer's `plans.<id>` is dropped |
| `plans.<id>.endpoint` | the firm's gateway: `https`, ASCII (punycode for a foreign host), a host, a port of at most 65535, no user name, password, query, fragment or backslash | none, so no endpoint the lawyer chose stands in for the firm's |
| `plans.<id>.provider` | `anthropic`, `bedrock` or `vertex` | none: the plan login; the `firm-policy` marker below refuses it in a firm |
| `plans.<id>.env` | extra variables, only these: `AWS_REGION`, `AWS_PROFILE`, `CLOUD_ML_REGION`, `ANTHROPIC_VERTEX_PROJECT_ID`, `ANTHROPIC_MODEL`, `ANTHROPIC_SMALL_FAST_MODEL`, `NODE_EXTRA_CA_CERTS`, and `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` in any case with no user name or password (`@`) in the value. No `*_BASE_URL` or `CLAUDE_CODE_USE_*`: the endpoint and provider set those | none |
| `install.bundle` | an offline bundle folder, an absolute path | unset |
| `install.registry` | a firm npm mirror: the same rule as an endpoint | unset: the pinned public hosts with their integrity checks |

All five are security keys. An unreadable source locks `plans.` (every `plans.<id>`, listed in
`SettingsView.locked` as `plans.`) and both `install` keys; a plan or install key a stronger
source set stands. A policy `plans` section that is no object (or a managed-preferences `plans`
that is no dictionary) locks `plans.` the same way, and so does a policy plan in a dotted shape
(`"plans.claude-acp"` at the top of `policy.json`, `"claude-acp.endpoint"` inside `plans`,
the registry value `plans.claude-acp.endpoint`), since a plan id takes no dot. A variable not on
the list, or a proxy with a login, is dropped with a warning naming it, in `settings.json` and
in every policy source alike; the rest of the plan loads. A name that is no variable name is
given by its place (`plans.claude-acp.env[2]`, in name order), never its text, which can be
`NAME=value`. No warning or error quotes a value: endpoint, mirror or variable.

The host writes these through `settings`: `Effective::set_plan` (refused with `settings.locked`
for a locked id, and whole for a banned name or a bad endpoint) and
`Effective::set_agent_launch`, which writes only Clauseo's own installs (a command under
`<data_dir>/agents/`, or a `node` or `node.exe` whose script is there), refuses an `env` that
sets a `KEY`, `TOKEN`, `SECRET` or `PASSWORD` name to anything but empty (keys live only in
the keychain), refuses a locked id, and leaves the lawyer's or the firm's own entry as it is.

`limits` takes the stored key at each read through `ProcessReaders::with_launch_env`, as
`ANTHROPIC_API_KEY` (Claude) or `CODEX_API_KEY` (Codex) over the entry's empty placeholder, so a
key sign-in reads as pay-as-you-go and the stderr tail redacts the key. `ProcessReaders::hold`
stops an agent's reads and resolves, once none runs, to a `HoldGuard`; the reads stay held
until every guard of that agent drops, so two overlapping holds need two drops and no hold
outlives the code that took it. The host (P4) keeps the guard across the whole leave order of a
sign-in, a sign-out or a key change, drops it, then sends `Input::LoginChanged`, which reads at once past
the back-off and reads again after a read that was in flight. `CODEX_OVERRIDES` gains
`cli_auth_credentials_store="file"` (the login stays in `CODEX_HOME/auth.json`, never the OS
keyring) and `shell_environment_policy.ignore_default_excludes=false` (Codex's shell tools do
not see `*KEY*`, `*SECRET*`, `*TOKEN*` variables). `claude_plan` and `codex_plan` are public
for desktop's sign-in line.

With the policy unreadable and the agent's `<data_dir>/agents/<id>/firm-policy` marker present,
sign-in and launch for that agent are refused with `signin.policy_unreadable`.

## Consequences

- Desktop owns the mapping from `agents` to the wire. A state or code added to `agents` needs a
  wire change here, and a new code needs a row in 0003 and the code owner's approval.
- A `Secret` cannot be logged by accident through serde or `Debug`. A future need to send one
  (a reconnecting client) needs a new decision, not a `Serialize` impl.
- `bindings.ts` changes when desktop regenerates it (P5a), and the UI's copy table needs words
  for the eleven new codes (the app thread).
- `SettingsView` literals gain `plans`; the host fills it in P3 from the `plans` section.
- **Not covered:** zeroing a `Secret` on drop does not reach the IPC buffer serde read it from
  (Tauri's, and serde_json's input); only Clauseo's own copy is zeroed.
- **Not covered:** with `cli_auth_credentials_store="file"`, Codex's login is plain text at
  `<data_dir>/agents/codex-acp/home/auth.json`. If `CLAUSEO_DATA_DIR` puts the data folder in a synced
  folder (OneDrive, iCloud, Dropbox), that file syncs; firm IT should keep the data folder out
  of sync roots.
- **Not verified:** that `shell_environment_policy.ignore_default_excludes=false` keeps
  `CODEX_API_KEY` out of Codex's shell tools is believed (`limits/src/readers/codex.rs:62-67`),
  not tested; G7 row 15 checks it.
- `setup` is outside `HostCommand`, so `cli` and `transport_ws` cannot install or sign in. A
  phone client that needs to would get its own record.

## Status

proposed (number confirmed by the host thread).

## Date

2026-11-04
