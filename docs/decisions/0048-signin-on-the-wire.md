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
`crates/desktop/src/setup/admit.rs` (P5a, one table keyed on the window labels) with a
refusal test each way:

| window | actions |
|---|---|
| `main` and `settings` | `InstallAgent`, `RetryAgentInstall`, `StartSignIn`, `CancelSignIn`, `PasteSignInCode`, `UseDeviceCode`, `OpenSignInPage`, `UseFoundLogin`, `UseDifferentAccount` |
| `settings`, and `main` for a first key | `SetAgentKey` |
| `settings` only (Plans) | `SignOut`, `ClearAgentKey`, `SetFirmEndpoint`, `FindLogin`, `StopUsingLogin` |
| `main` only | none |

A refused action answers `transport.unauthorized` (`dispatch` and `query` refuse a window with
`command.invalid`; `setup` keeps this record's code, as a declined confirm answers).

*`main`'s first key* (the orchestrator's ruling for UD1's "Use an API key"): `main` may send
`SetAgentKey` only while that agent has no key stored. The backend checks and stores in one
step (`only_if_absent`), so two quick sends, or `main` racing `settings`, store one key; the
other answers `settings.key_stored`, which the app words as "open Settings, then Plans".
Replacing or clearing a key stays the settings window's. The flag is `MAIN_SETS_FIRST_KEY` in
`admit.rs`. Residual risk: script running in a compromised `main` could plant its own key on a
fresh install, with no key stored yet; it can never replace or clear one.

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

**The host's settings writes and launch (R20 P4a).**

- `HostHandle::set_agent_launch(agent, AgentLaunch) -> Result<EntryWrite, HostError>` and
  `HostHandle::set_plan(agent, Option<Plan>) -> Result<(), HostError>` are Rust only. They run on
  the actor through `Effective::set_agent_launch` and `Effective::set_plan`, the same way
  `SetUiSettings` writes `settings.json`, and send `SettingsChanged` after a write.
  - `Error::Locked` maps to `settings.locked`.
  - `Error::Refused` maps to the new `settings.refused`, which is not retryable and is placed
    just before `unknown`. Its message is the settings crate's: the key, or a variable's place,
    and never a value.
  - `EntryWrite::LeftAlone` is `Ok`, with nothing written or sent.
- The host fills `SettingsView.plans` from the `plans` section, with names only.
- Each agent with a kind gets Clauseo's own home, `Locations::agent_home` of
  `AgentKind::id()`, made at the launch and forced over every other source: `CODEX_HOME` for
  Codex, `CLAUDE_CONFIG_DIR` for Claude Code. An inherited `CLAUDE_CONFIG_DIR` no longer reaches
  a Claude agent.
  - The plans folder for plan notes is read from the launch's merged environment, after this
    variable is in it.
  - The profile's Codex skills go in `skills` of the same Codex home
    (`clauseo_profile::paths::codex_skills`).
  - Claude's skills stay in Clauseo's plugin, passed by path. The forced-ask rules are
    flag-tier settings, not a file. Neither depends on the home.
- Codex's launch config takes both new overrides from the one `clauseo_limits::CODEX_OVERRIDES`.
- The host drops codex-acp's `logout` from a Codex agent's `CommandsAvailable` and refuses a
  prompt whose text starts `/logout` with `command.invalid`. It does this whatever the login's
  source, since signing out goes through `SignOut` alone.

**The host's sign-in calls, hold and hook (R20 P4b).**

- *New `HostHandle` calls*, all Rust only:
  - `restart_agents(agent) -> Result<(), HostError>` stops each live agent of that id that is
    not in a turn, with the shutdown ladder's stop, so the next question launches with the new
    login. It then resumes each queue held for that agent's sign-in, once.
  - `stop_agents(agent) -> Result<(), HostError>` is step 1 of the leave order. It cancels each
    turn of that id through the Cancel path (the lawyer's Stop, so what the turn left running
    ends too) and marks the turn to go back held. It drops a launch still being prepared, whose
    waiting work fails as closed, and stops every live agent of that id.
  - `set_reused_login(agent, bool) -> Result<(), HostError>` records desktop's word on the
    login's source. While it is `true`, the split check runs at each Codex launch.
  - `login_splits() -> async_channel::Receiver<LoginSplit>` carries `LoginSplit { agent,
    lawyers_file_gone }`, sent when a split check refuses a launch. Desktop runs
    `stop_using_login` with `CodexLinkSplit`, or `ReusedSignedOut` when the file is gone, then
    the leave order. The host has set its own reused flag to `false` by then.
  - `remove_login_links(agent) -> Result<(), HostError>` asks the actor only for the folders.
    The removal of each `auth.json` runs on a `clauseo-codex-fs` thread with a 5 s wait, never
    on the actor. Before stage C (R17) no matter has a link, so the list is empty.
- *The sign-in hold.* A turn is held for its agent's sign-in when it fails with
  `agent.auth_required`, or with an error the sign-in-again test accepts, which then ends as
  `agent.auth_required`. A turn `stop_agents` cancelled is held the same way, unless it ended
  `EndTurn`. Once its close is durable, the turn goes back to the head of its queue as one commit
  of a new `UserMessage { Queue }`, with the same content, and its `TurnQueued` behind the ended
  turn. These are the entries any queued prompt has. The queue is held through the same flag
  as after any stop but `EndTurn`, a refused permission's `NotAllowed` (#408) included. So
  `queue_held` and the queued prompt look the same whatever held them, and the app's held lines
  (#416) need nothing new. Actor state alone marks the queue as waiting for the sign-in:
  `restart_agents` clears the mark and resumes the queue, so a second call does nothing. A
  lawyer's `ResumeQueue`, `SendQueuedNow` or new prompt moves the queue as usual, and the mark
  goes once the queue no longer shows held. Nothing new is logged for the hold beyond one flag.
- *The held turn keeps its head place across a host restart* (the host code owner's ruling,
  2026-11-05). It was sent first, and the turns queued behind it may depend on its answer. Its
  `TurnQueued` carries `at_head: true`, a new optional field (`TurnQueued::at_head`,
  `is_at_head`). False is written as a missing field and a missing field reads as false, so
  every older entry, and every other `TurnQueued`, is written byte for byte as before, the
  corpus is unchanged, and an old log restores in log order as before. Start-up's queue rebuild
  (`queue/restore.rs`) puts the turns marked `at_head` first, the latest put back first, then
  the rest in the order they joined the queue. When the first restored turn is one of them, the
  queue comes back held for the sign-in as well as held, so `restart_agents` resumes it as it
  would have before the restart.
- *Every view shows that order* (the host code owner's review of #440). The reducer marks the
  prompt of a turn whose `TurnQueued` is `at_head` with `UserMessageEntry::at_head`, a new
  optional field written only when true, so older views and snapshots read as before.
  `clauseo_reducer::queued_order` orders a thread's queued prompts as the host runs them: the
  `at_head` ones first, the latest put back first, then the rest by `order`. `bindings.ts`
  gains `at_head?: boolean`, and the app's queue strip (`room/queue-picks.ts`) reads it with
  the same rule, so after a restart the strip shows `[again, two, three]` as the host sends them.
  A view with no `at_head` orders by `order` exactly as before. A requeued steer still takes
  the head live while the view shows it in its message's place; that is left to the host
  thread to decide, since a restart restores a steer in log order by an earlier ruling.
- *The `LoginCheck` hook.* `HostConfig::with_login_check(Arc<dyn LoginCheck + Send + Sync>)` sets
  it. Its one call is `check(&AgentId, agent_home: &Path) -> LoginLink`, and `LoginLink` is
  `Own`, `Shared`, `Split` or `Gone`. Desktop backs it with S18's `codex_link_holds` and
  `codex_login_shared`. `HostConfig::with_sign_in_again(fn(&str) -> bool)` sets the
  sign-in-again test (S18's `is_sign_in_again`) apart from the hook, so the hook's inputs stay
  the agent and its home. If neither is set, nothing is checked: there is no split check, no
  guard, and only `agent.auth_required` holds a turn.
  - The host asks the hook at each Codex launch, after the launch's other checks pass. The call
    runs off the actor on a `clauseo-codex-fs` thread, and the host waits at most 5 s on its
    clock.
  - While the login is reused, any answer other than `Shared` refuses the launch with
    `agent.auth_required`, so its turn is held. The host also sets the reused flag to `false`
    and sends a `LoginSplit`.
  - When the check does not answer in time, or its thread fails, that is never a sign-in
    answer. No split is sent, no flag changes and no turn is held. The brief's rule for an
    unknown answer is to refuse, as S18's stat failures do. So while the login is reused the
    launch fails with `agent.launch_failed`, which may be retried, and the guard treats the file
    as shared.
  - *The guard.* While the hook is set and the login is reused, or the last check answered
    `Shared` or gave no answer, a prompt to that Codex agent whose text starts `/login` is
    refused with `signin.login_shared` before anything is queued. `/logout` is refused always,
    as above. The host never calls `login`.
  - *`/login` is hidden too.* The host drops `login`, in any case, from the commands a Codex
    agent advertises, beside `logout`, before they are logged as `CommandsAvailable`. Signing
    in goes through Clauseo's own sign-in. A typed `/login` is still refused by the guard as
    above, and otherwise goes to the agent as before.
  - The hook's logs name the agent and the outcome, never a path.
- *The API-key `authenticate` retry* (brief, "API keys"). When a Codex agent answers
  `session/new`, `session/load`, `session/resume` or `session/prompt` with auth-required,
  the host calls `AcpClient::authenticate("api-key")` once and sends the same call again, only
  when all of these hold:
  - the profile hook's launch env set a non-empty `CODEX_API_KEY` for that launch;
  - the launch's `LoginCheck` answered `Own`. `Shared`, `Split`, `Gone`, no answer and no
    hook set never retry;
  - desktop does not say the login is reused (`set_reused_login`), checked again at each call,
    so the host never retries while a link exists.

  There is one try per agent process (`actor/turn/key_retry.rs`). A second auth-required, or
  an `authenticate` that fails, ends the call with the first auth-required, so the turn is held
  for the sign-in with `agent.auth_required` as above. The host sends only the method id, and
  codex-acp reads the key from its own environment, so the host never reads the key's value.
  The logs carry the outcome and an error's code only.
- *New codes.* `settings.refused` (P4a, above) and `signin.login_shared`. The second is not
  retryable and goes just before `unknown`, after `settings.refused`. It refuses a Codex login
  change while Clauseo's Codex home shares the lawyer's ChatGPT login file. The fix is in the
  app ("Stop using it here"), and the words are in `ERROR_WORDS`.

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
- **Deliberate:** the allowlist takes `NODE_EXTRA_CA_CERTS` together with an `http://` proxy,
  which lets that proxy read the agent's TLS traffic; firms with TLS-inspecting proxies need
  it. Only someone who can already write the plan (the firm's policy, or the lawyer) can set it.
- **Not verified:** that `shell_environment_policy.ignore_default_excludes=false` keeps
  `CODEX_API_KEY` out of Codex's shell tools is believed (`limits/src/readers/codex.rs:62-67`),
  not tested; G7 row 15 checks it.
- `setup` is outside `HostCommand`, so `cli` and `transport_ws` cannot install or sign in. A
  phone client that needs to would get its own record.

## Status

proposed (number confirmed by the host thread).

## Date

2026-11-04
