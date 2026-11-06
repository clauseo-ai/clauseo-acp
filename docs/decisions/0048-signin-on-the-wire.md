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
| `StartSignIn { console }` | the plan login (`console`: Claude's Console account); joins a sign-in already running; `command.invalid` for a reused login, which is left first |
| `CancelSignIn` | stops the running sign-in |
| `PasteSignInCode { code: Secret }` | gives the running Claude login the pasted code |
| `UseDeviceCode` | switches Codex to its device code; `command.invalid` for a reused login |
| `OpenSignInPage` | opens the current page again ("Didn't open?"); nothing in any other state |
| `SignOut` | signs Clauseo's own login out; `command.invalid` for a reused login |
| `UseFoundLogin` | "Use it": reuses the login `LoginFound` showed; refused in any other state |
| `UseDifferentAccount` | leaves the found or reused login and signs in |
| `StopUsingLogin` | "Stop using it here": stops reuse, never a logout |
| `FindLogin` | looks again for a login the lawyer already has; ends in one `SignInChanged`, sent even when unchanged (`Failed { signin.failed }` when the search cannot tell) |
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
  `Failed { code, at }`, and desktop's own `Switching { since, stuck }` (P5b, below). `found`
  is a `FoundLogin` (`claude_settings`,
  `claude_windows_sandbox`, `codex_keyring`, `reused_failed`, `reused_signed_out`,
  `reused_changed`, `codex_link_split`, then UD2's `none_found`: the search ran and found no
  login, and `reuse_off`: S18's `no-reuse` is there, so nothing was looked for); `from` is a `LoginFrom` (`claude_code`, `codex`);
  `method` a `SignInMethod` (`plan`, `api_key`, `firm`); `source` a `SignInSource` (`reused`,
  the lawyer's own CLI login, or `own`, Clauseo's). `plan` is a `PlanLabel` in words, from
  `limits`' plan words; desktop turns S18's raw plan string into it. Times are `Timestamp`s. A
  device code is shown to the lawyer, so it is a plain string, not a `Secret`.

A second `FindLogin` replaces the first (`agents` `signin/job.rs`, `Task::supersedes`), and two searches in flight can end in one `SignInChanged`, because compose keeps one waiting flag per agent; the UI reads the latest state, so that is harmless.

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
  - `stop_agents(agent)` is step 1 of the leave order (P4d, below, has its answer and hold). It cancels each
    turn of that id through the Cancel path (the lawyer's Stop, so what the turn left running
    ends too) and marks the turn to go back held. It drops a launch still being prepared, whose
    waiting work fails as closed, and stops every live agent of that id. `stop_agents_for(agent,
    StopFor)` does the same with a reason: `StopFor::LoginSwitch` (what `stop_agents` gives) shows
    the held queues as `switching`, and `StopFor::SignInAgain`, for an account change on the shared
    login, shows them as `sign_in`, so the app asks for a sign-in; the questions stay held either way.
  - `set_reused_login(agent, bool) -> Result<(), HostError>` records desktop's word on the
    login's source. While it is `true`, the split check runs at each Codex launch.
  - `login_splits() -> LoginSplits` carries `LoginSplit { agent, lawyers_file_gone }`, sent
    when a split check refuses a launch. Desktop runs `stop_using_login` with
    `CodexLinkSplit`, or `ReusedSignedOut` when the file is gone, then the leave order. The
    host has set its own reused flag to `false` by then, so the latest split is all desktop
    needs: `LoginSplits` keeps the latest unread split per agent, a newer one replacing it in
    its place, so a slow reader loses none and none back up (the host code owner's review of
    #440; it was a channel of 16 that dropped extras). Its `recv` and `try_recv` answer as
    `async_channel`'s do, and every clone takes from the same splits.
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
  lawyer's `ResumeQueue` or `SendQueuedNow` moves the queue as usual, and the mark goes once
  the queue no longer shows held. A new prompt sent while nothing runs queues at the end instead,
  held, whatever its delivery (send now and a steer with nothing to join included), and a prompt
  joining the queue keeps it held and marked (the host code owner's review of #461). If the lawyer removes the turn put back, the mark goes too (the host
  code owner's ruling on #440): the queue stays held the usual way (#408), so the next turn
  waits for the lawyer's "Send next", and `restart_agents` sends nothing. Nothing new is logged for the hold beyond one flag.
- *The held turn keeps its head place across a host restart* (the host code owner's ruling,
  2026-11-05). It was sent first, and the turns queued behind it may depend on its answer. Its
  `TurnQueued` carries `at_head: true`, a new optional field (`TurnQueued::at_head`,
  `is_at_head`). False is written as a missing field and a missing field reads as false, so
  every older entry, and every other `TurnQueued`, is written byte for byte as before, the
  corpus is unchanged, and an old log restores in log order as before. Start-up's queue rebuild
  (`queue/restore.rs`) puts the turns marked `at_head` first, the latest put back first, then
  the rest in the order they joined the queue. When a restored turn marked `at_head` is a turn held
  for the sign-in, the queue comes back held for the sign-in as well as held, so `restart_agents`
  resumes it as it would have before the restart. A requeued steer is marked `at_head` too, and
  keeps the head place, but it does not mark the queue as waiting for the sign-in.
- *Every view shows that order* (the host code owner's review of #440). The reducer marks the
  prompt of a turn whose `TurnQueued` is `at_head` with `UserMessageEntry::at_head`, a new
  optional field written only when true, so older views and snapshots read as before.
  `clauseo_reducer::queued_order` orders a thread's queued prompts as the host runs them: the
  `at_head` ones first, the latest put back first, then the rest by `order`. `bindings.ts`
  gains `at_head?: boolean`, and the app's queue strip (`room/queue-picks.ts`) reads it with
  the same rule, so after a restart the strip shows `[again, two, three]` as the host sends them.
  A view with no `at_head` orders by `order` exactly as before. Until this ruling a requeued steer
  took the head live but its view showed it in its message's place, and a restart restored it in
  log order. The host code owner's follow-up ruling: a requeued steer is queued `at_head` too
  (`TurnQueued::at_head` on the `TurnQueued` behind the steered turn), so a restart keeps it at the
  head as it runs live, and the view orders it first, the same as the host.
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
  - *The guard.* While the hook is set and the login is reused, or the last check found the
    file shared or gave no answer (P4d: `shared`, below), a prompt to that Codex agent whose text starts `/login` is
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

**The host's additions for desktop's leave flow (R20 P4d, the host code owner's approval,
2026-11-05).** All Rust only.

- *`limits_login_changed(AgentKind) -> Result<(), HostError>`* is `limits`' `Input::LoginChanged`
  (P2): one read at once, past any age or back-off, and the next scheduled read where it was.
- *`stop_agents` waits, bounded, and holds.* It now returns `Result<AgentsStopped, HostError>`.
  It answers `Exited` once every process of the id has ended, and `TimedOut` once its own 10 s
  wait on the host clock has run out (the shutdown ladder's 7 s and a margin), or when a process
  outlived its kill. It never hangs. From the call on, the id is held: nothing of it launches,
  and every prompt sent while nothing runs queues instead, at the end, whatever its delivery
  (queue, send now, or a steer with nothing to join), held and marked for the sign-in as P4b's
  turn put back is (#408's hold), so it is not lost and nothing fails. A prompt joining the queue
  keeps it held and marked (the host code owner's review of #461). A launch that still comes (work
  waiting on a stopped agent) fails with `agent.auth_required`, so its turn goes back held.
  A lawyer's `ResumeQueue` or `SendQueuedNow` under this hold is accepted and changes nothing: the
  queue stays held and marked, its held flag still shows, and its turns run once, in order, at
  `restart_agents` (the host code owner's review of #461).
  `restart_agents` lifts the hold and resumes once, through P4b's path. Desktop calls it too
  when its leave flow is abandoned, so there is no `release_agents`. **The hold lives in memory
  only:** a host restart clears it. The queue then comes back held as every restored queue does,
  and waits for the lawyer, not for `restart_agents`.
- *Holds compose (R23 PR 4, the code owner's ruling).* A thread's holds are a set, the sign-in's
  (`SignIn`), `stop_agents`' (`Stop`) and an install's under `now` (`UpdateNow`, 0037); a
  prompt sent while any applies takes one held append (`queue/hold.rs`), each lift removes only
  its own reason, and nothing starts while `Stop` or `UpdateNow` is left. `SignIn` holds through
  the queue's held flag, as any held queue does, which a head sent now passes; otherwise a queued
  prompt stays held while any hold applies. With the sign-in's among them it queues as above.
- *`set_launch_refused(agent, Option<ErrorCode>) -> Result<(), HostError>`* refuses the id's
  **new launches only**. The codes come from a list: `signin.policy_unreadable`,
  `agent.keys_loading` and `agent.key_unreadable`. Any other code is a bug: debug builds
  assert, and release builds refuse it with `command.invalid` and change nothing. A turn that would launch fails as any refused
  launch does, an ordinary `TurnFailed` with that code. **Sessions already running go on until
  they exit,** unless desktop calls `stop_agents`. A refusal applies to new launches; a running
  session ends normally. `None` lifts the refusal. It is in memory only.
  `HostConfig::with_launch_refused` sets the same refusals before the host starts, so none of
  them depends on a later call (the security review's M3): desktop starts the host with
  `signin.policy_unreadable` for each agent an unreadable policy blocks and `agent.keys_loading`
  for every other known agent, and lifts the keys' refusal once the keys have loaded; the
  policy's stays until the settings lift it. A launch meeting `agent.keys_loading` waits at most
  2 s on the host clock for it to be lifted, then checks the refusals again: still refused, it
  fails with that code and no process starts (ruling 4: never launch without the key).
- *`agent.keys_loading` and `agent.key_unreadable` are two different facts* (R20 P5b A4, the
  orchestrator's ruling). `agent.keys_loading` means desktop has not finished loading the stored
  keys at all: start-up. `agent.key_unreadable` means this agent's stored key was tried and the
  read failed (the keychain's error, such as a refused prompt) or did not answer in time. Both
  are retryable, and a launch waits out either for 2 s.
  - *The first load is bounded.* Desktop reads each agent's key on a thread of its own and its
    setup thread waits for them at most 10 s on its timer (`setup/keys/reads.rs`). Then, per
    agent: no item stored lifts the refusal, and it launches without a key; a key read fine lifts
    it, and it launches with the key; a read that failed sets `agent.key_unreadable`. Such a
    key never reads as "no key" and the agent never launches without it. A read that has not
    answered by the bound sets `agent.key_unreadable` only for an agent the "key stored" marker
    below marks; an unmarked agent has no key Clauseo stored, so it launches without one and no
    refusal follows. The log line names the agent, "key unreadable" and the error's kind; never
    the key, the item's name or a path.
  - *The "key stored" marker* (the orchestrator's ruling, a follow-up to #506). Desktop keeps,
    outside the keychain, a list of the agents Clauseo stored a key for:
    `<data_dir>/agents/key-stored.json`, `{"agents": ["claude-acp"]}`, beside the `firm-policy`
    markers and written as they are, whole and atomic (`setup/keys/marker.rs`). It holds agent
    ids only: no key, no keychain item name. A save sets the marker before the keychain write,
    and a marker that cannot be written refuses the save (`internal`, nothing stored); a
    keychain write that then fails leaves it on. A clear removes the key first, then the
    marker; a marker that cannot be cleared stays on. Each keychain read that answers (the
    first load, a late read, a read again, a probe, and the reload a refused first key takes)
    brings it in step, best effort: a key sets it, no item clears it.
  - *When the keychain never answers by the bound,* an agent the marker marks is refused with
    `agent.key_unreadable`, and one it does not mark launches without a key: Clauseo never
    stored one for it. No marker file (a fresh install) marks no agent; a file that cannot be
    read or does not parse marks every agent (fail closed), logged by the error's kind only.
    A read that fails with the keychain's error still refuses the agent, marked or not.
  - *The residual.* A key stored by a build without the marker, or removed outside Clauseo,
    leaves the marker stale until a read answers. A stale "on" only refuses the agent, so it
    fails closed; "off" with a key stored can only happen for a key Clauseo never stored, or
    one stored by a build before the marker (dev builds only: no released install predates
    it). A key a pre-marker dev build stored launches keyless on the first launch after the
    upgrade if the keychain is silent then, since the marker only appears once a read answers;
    a missing marker file still marks no agent, because treating it as all marked would refuse
    a fresh plan-only install on a slow keychain. A marker's folder that will not sync after
    its rename (rare, an I/O error) is logged, not refused: the rename has already landed.
  - *What the window shows.* An agent whose key is unreadable shows `Failed { code:
    agent.key_unreadable }`, never "signed in with an API key", and its sign-in is not barred
    with `signin.key_in_use`: a sign-in may run, and launches stay refused until a read
    succeeds. The launches, the restart after a leave and a key's overwrite still count an
    unreadable key as stored.
  - *The retry.* A read that had not answered goes on, and when it answers it is taken in: a key
    or no item lifts the refusal. A launch meeting `agent.key_unreadable` also asks desktop to
    read that key again (`HostConfig::with_launch_retry`, with the code; called on the actor,
    never blocking)
    before its 2 s wait, unless a read of it is still running. A read again that succeeds lifts
    the refusal, so that launch, or the lawyer's next try, goes on. A read that fails again
    leaves it. A key the lawyer sets or clears also ends the mark. Desktop keeps one refusal in
    force per agent (`setup/refusal.rs`): the policy's, else the unreadable key's, else none.
  - *A setup thread that never loads the keys.* The host bounds `agent.keys_loading` itself: from
    12 s after its start (desktop's 10 s and a margin), one still in place counts as
    `agent.key_unreadable`, for every agent it covers, since only desktop knows which have a
    stored key. So launches never stay "still starting" for good.
  - *Limits reads wait for the keys.* Before the keys load, a limits read waits (at most 12 s,
    then fails) and launches nothing, so it never reads another login's numbers for an agent
    whose key is stored. An agent whose key is unreadable fails its reads.
    An agent an unreadable policy blocks (`signin.policy_unreadable`) fails its reads too, so
    a stored key, perhaps a firm gateway's, never goes to the vendor's own host.
  - *The refusal never lags the key.* Each `restart_agents` desktop makes sets the agent's
    refusal first, so a key that just turned unreadable is refused before the restart's
    launches. A `set_launch_refused` call that fails is tried once more, then logged by the
    agent and the error's code.
- *`LoginCheck::shared(&AgentId, agent_home: &Path) -> Shared`*, a second hook call, where
  `Shared` is `Yes`, `No` or `Unknown`. It asks whether the home's `auth.json` is the lawyer's
  file, whatever the link check says. A symlinked `auth.json` is `Split` to `check` (it is no
  hard link) and `Yes` to `shared`. The host asks both at each Codex launch, in the same
  bounded off-actor call. **`Unknown`, and a call that does not answer in time, count as
  shared** (fail closed), decided in one place (`login_link::treated_as_shared`). Only the
  `/login` and `authenticate` guard reads `shared`: the guard refuses `/login`, and the API-key
  `authenticate` retry is not armed, while the login is reused or `shared` counts as shared.
  The split check still reads `check` alone.

**Desktop's leave flow (R20 P5b; the orchestrator's rulings and the host code owner's
requirements, 2026-11-05).** Desktop runs ruling 1's leave order (`setup/leave.rs`): the reads'
hold, `stop_agents`, S18's `stop_using_login` and its `SignedOut` (30 s at most), then
`set_reused_login(false)`, `remove_login_links` (Codex) and the entry rewritten `Own`, then the
reads' hold dropped and `limits_login_changed`.

- *One leave per agent.* A second leave of an agent while one runs answers `command.conflict`.
  The leave's waiter on S18's next resting state is its own: taking it back never removes a
  waiter put after it, and a state S18 sent before the waiter was put never fills it. A change
  from `SignedIn { Reused }` to any other resting state with no leave waiting (a late
  `SignedOut`, or S18 ending the reuse by itself) runs the last three steps at once. A new
  `SignedIn` that lands while a leave waits is never swallowed (review A1): the pump handles it
  first (the host told the new source, the entry rewritten, `restart_agents`, the limits told),
  then ends the leave with it. A lawyer's leave then answers done; an automatic one stops.
- *A lawyer's leave* (`StopUsingLogin`, `UseDifferentAccount`, `SetAgentKey` or
  `SetFirmEndpoint` on a reused login) whose `stop_agents` times out, or whose call fails,
  changes nothing: `restart_agents` lifts the hold, the reads' guard drops, and the command
  answers `signin.agents_still_running` or the call's error. The queues then go on as
  `restart_agents` resumes them. With no `SignedOut` within 30 s **the hold stays** (review
  A2): S18's stop job may still end the reuse, and S18 has no cancel for it (`cancel_sign_in`
  ends only a sign-in job). The agent shows `Switching { since, stuck }` from then (`stuck`
  after 2 minutes). `StopUsingLogin` answers done; `UseDifferentAccount`, `SetAgentKey` and
  `SetFirmEndpoint` answer `command.conflict` ("still leaving the old login; try again in a
  moment") with nothing stored or started, since the late end could rewrite the entry under
  their change, or fail. When the job ends, the pump runs the last three steps if the reuse
  stopped (never on a `Failed`: the stop failed, and nothing else changes), `Switching` ends,
  then the hold lifts (`restart_agents`) if the stop failed or the reuse holds, or follows the
  rule below if the reuse stopped, and the reads' guard drops; meanwhile another leave of
  the agent answers `command.conflict`. Nothing changes under running agents. **A stop job
  that never reports keeps the agent held until the app restarts.** A `Failed` while the login
  is reused never counts as the reuse's end. Whether the login is reused is desktop's word,
  as the host was told, never S18's last state, so a running state between hides nothing; a
  `Failed` that follows a running state (a job that replaced the stop) leaves the reuse
  unknown, and S18's status check decides, the holds waiting for its answer. A leave dropped before it ends (a panic, or its command
  dropped) lifts the hold from a drop guard. A failed `restart_agents` is tried once more, then
  logged as a warning. A leave that finishes follows the rule below; one that is the first
  step of `SetAgentKey` or `SetFirmEndpoint` leaves the restart to its command, which calls
  `restart_agents` once the key or the endpoint is stored (or by the rule, should the store
  fail), so the agents restart once and never before the new key or endpoint is in place.
- *An automatic leave* is a split the host's check found, or a reuse S18 ended by itself:
  `ReusedFailed`, `ReusedChanged`, or `ReusedSignedOut` from its status check. It runs on a task
  of its own, never inside the pump. After `ReusedChanged`, no agent goes on with the account the
  CLI now holds: the entry is rewritten `Own` at once, and the leave stops the agents. A try that
  does not finish (`stop_agents` timed out, no `SignedOut` within 30 s, or a call failed)
  **keeps the hold**, so nothing launches on the old or changed login or on a split copy of the
  lawyer's file. Questions sent meanwhile queue held and marked (P4d). It tries again after 5 s,
  30 s, then every 5 minutes, until a try finishes or a new `SignedIn` arrives. It never gives
  up. A try that finishes, the first or a try again, follows the rule below. An
  automatic leave refused because another leave runs is run again when that one ends without
  leaving. **A new login ends it, never the other way round** (reviews A1, H1′): before its
  first try, before each try again, and right before `stop_using_login` (under the sign-in
  gate), it checks for Clauseo's own login signed in, or a new `SignedIn` since it began (the
  pump has handled it, or S18 has sent it and the pump has not taken it yet, such as a "Use it"
  that finished as `SignedIn { Reused }`); then it asks S18 nothing more and ends, and
  `Switching` with it. The hold is then the new login's to lift: its `SignedIn`'s
  `restart_agents`, or the leave's own once more when the pump had handled it. A sign-in S18
  runs or has reserved (`Agents::sign_in_going`) only skips the try, so a try never cancels it:
  the agent stays held and the sign-in shows over `Switching`; should it fail, `Switching`
  shows again and the next try runs, so no hold is silent. A panic in an automatic leave lifts its hold and ends `Switching`,
  and the setup thread's other followers go on (review A3; release builds abort on a panic).
- *After any leave that finishes* (an automatic one's first try or a try again, a lawyer's,
  or a lawyer's that gave up and whose stop job then ended the reuse; the orchestrator's
  ruling, 2026-11-06), one function decides (`setup/leave/after.rs`, `usable_after_leave`):
  when a usable login remains for the agent, a stored API key, an endpoint or a provider (the
  lawyer's `plans.<id>` or the firm's policy's, what makes a sign-in answer
  `signin.endpoint_in_use` or `signin.firm_managed`), or Clauseo's own login signed in,
  desktop calls `restart_agents` once, and the held questions run on it. Otherwise it does not:
  the queue stays held for the next login, whose `SignedIn` the pump restarts on (or the key or
  endpoint command, once stored). That hold is never silent: with no key or endpoint the
  composed state has no `ApiKey` or `Firm` line, so the agent shows S18's `SignedOut`. A leave
  that starts while an endpoint restart is in its own host awaits can let agents launch on the
  reused login against the endpoint, within two host round-trips, until the new leave's
  `stop_agents` holds them; this is a tracked follow-up.
- *`SignIn::Switching { since, stuck }`* is a new variant of the closed `SignIn`, shown from the
  first try's end. `since` is that moment, and a try again never moves it. `stuck` turns true 2
  minutes after `since`, on the timer S18's `Waiting { nudge }` runs on. A running sign-in still
  shows over it, and the leave's own failures never do. It ends when a try finishes, and the
  state after it shows, or when a new `SignedIn` arrives. **It has no `Failed` exit.** The hold
  lives in memory only: quitting Clauseo clears it, and the next start's S18 status check finds
  the split or the changed login and runs the leave again. **A process that never exits keeps
  questions waiting until the app restarts.** The app gives the card its words later.
- *Clauseo's own login signed out* calls `restart_agents` as well as `limits_login_changed`
  (ruling 3), so no idle agent stays on the old login.

**Identity check (R17; the orchestrator's ruling, agreed with the host code owner,
2026-11-06).** Every matter's agents use the one login, which stays writable for token refresh,
so an agent in one matter could swap another account into it and another matter's work would
go there. Desktop records which account each agent's login belongs to and acts when that
changes (`setup/identity.rs`).

- *Where the identity comes from.* Only the agents' own status output: S18's `SignedIn {
  email }`, and `Agents::read_account` (Claude's `auth status --json`, Codex's `login status`
  then `account/read`, under the environment a session of that login sees; it changes no
  sign-in state). Never a token or a credential file's contents. The email names the account;
  with none, the account id does, when the status gives one (Claude's `accountUuid`; Codex's
  `account/read` has no id at the pinned schema), so an id is enough. It is kept only as the
  SHA-256 of that identity (trimmed, lower-cased), in `<data_dir>/agents/identity.json` (agent
  id to hash, written atomically, not at all when unchanged), so a swap while Clauseo is closed
  shows at the next start. A `SignedIn` with no email cannot tell, so it asks for a check, whose
  status may give the id; a sign-in through Clauseo with no email drops the old record and the
  check records the first account it reads, by id or email, even after a launch (it awaits its
  record for the sign-in's bound; past it, unknown and checked). **Neither an email nor an id is unknown** (refused, retryable).
- *When it is recorded.* At a sign-in or reuse made through Clauseo (`StartSignIn`,
  `UseDeviceCode`, `UseFoundLogin`, `UseDifferentAccount`): its `SignedIn` records first, before
  anything compares it, so a new account chosen in Clauseo never reads as a swap. The sign-in
  counts as under way for its run's own bound at most (`clauseo_agents::sign_in_limit`: 10
  minutes for Claude, 20 for Codex's device code), moved on at each running state, each time
  with the login files' stamps then. No check runs while it is under way, but the gate and the
  watcher keep their stat: a change of those stamps sets `agent.identity_checking` (on a verdict
  that refused nothing) and holds it until the sign-in's record lands, which holds for the
  stamps from before its confirming status read, so the next look checks the write. A record
  that never lands, because the bound passes first, gives `agent.identity_unknown` and a check. **Any other `SignedIn` is compared:** one from S18's status
  check (at start, or when a window shows Plans) naming another account is a change. Before R17
  the pump took such a state as a new sign-in and restarted the agents on it.
- *First run: trusted on first use, only before any launch.* With no `identity.json` (every
  upgrade to R17) every agent starts refused with `agent.identity_checking` until its startup
  check, which the follower runs as soon as setup begins, records the account it names (or, with
  none, leaves it idle). Once a launch of the agent has passed the launch gate, an account with
  no record is never recorded: it is unknown, and refused. **Residual:** a swap made before
  Clauseo first starts with R17 is trusted. That is outside the threat model, which covers an
  agent running inside Clauseo.
- *The cache.* The verdict holds while the login files' stamps are unchanged: the file id, the
  last write, the last status change (ctime with nanoseconds on Unix; `ChangeTime` on Windows)
  and the size, read from metadata only. Never a digest of the file's bytes, not even in memory
  (ruling 4). The stamps are taken **before** the status read the verdict comes from, and a
  record keeps them: a sign-in's from before its last step, a check's from as it began. With an
  account recorded and no stamps held (at start), the next look checks; it never takes what is
  there as the baseline. Codex: `auth.json` in Clauseo's agent home, and the lawyer's
  `~/.codex/auth.json` while reused. Claude: the store file `CLAUDE_SECURESTORAGE_CONFIG_DIR`
  points a session at, `<home>/.credentials.json` for Clauseo's own login and the lawyer's
  `~/.claude/.credentials.json` for a reused one (Linux and Windows keep the login there). On
  macOS Claude keeps it in the Keychain, which no stat sees, so with an account recorded a
  macOS Claude login is due a check again every 10 minutes, files or not (a session may plant a
  store file), before the next launch. **Open point:** a Keychain-only Claude login fails inside
  the macOS sandbox, so the host will hand Claude its login through `CLAUDE_CODE_OAUTH_TOKEN` or a
  Clauseo-owned file store, not yet decided; the check runs Claude's status with the store the
  launch entry names and watches whichever store file exists there.
- *When it looks.* At each launch, on the host's prepare thread, through a new Rust-only hook,
  `HostConfig::with_launch_gate` (R17 review M3): desktop stats the agent's login files and
  compares them with the stamps held. Fresh stamps answer `None` with no other work (a launch
  costs one stat per login file). A difference makes the verdict due, asks for the check, and
  sets `agent.identity_checking` through `set_launch_refused` (awaited) before the gate answers
  it; the host then waits it out as below. Between launches, a stat every 2 s, sessions running
  or not, signed in or not while an account is recorded. Each agent's check runs on its own, so
  one status run that hangs to its bound holds up no other agent.
- *The flow.* A stamp changes: `agent.identity_checking` at once (waited out like
  `agent.keys_loading`, up to 2 s, woken when it lifts; a launch meeting it asks desktop through
  the host's launch retry, `HostConfig::with_launch_retry`, which passes the code:
  `agent.key_unreadable` reads the key again, `agent.identity_checking` runs the check; retries
  queued together ask for one check), then the status, bounded by 20 s. The same account lifts
  the identity's refusal and no other (a policy's or an unreadable key's stays in force over
  it); a status that failed, timed out or named no account while one is recorded sets
  `agent.identity_unknown` (checked again every 30 s only after a status that failed or timed
  out; otherwise when a login file changes); after a sign-out through Clauseo, no account is
  the signed-out state instead (below); another account sets
  `signin.account_changed`, then `stop_agents_for(agent, StopFor::SignInAgain)`, then shows
  `SignIn::Failed { code: signin.account_changed }`. The host's hold stays until a sign-in
  through Clauseo records an account again and `restart_agents` runs, so a prompt to the agent
  meanwhile, in any thread, queues held as `sign_in` rather than failing, and runs then. A reused
  login also gets S18's status check, which ends the reuse as `ReusedChanged`. A refusal the host
  cannot be told (the call fails twice) is logged as an error and told again at each look until
  it lands; the stop's hold covers a changed account meanwhile.
- *A sign-out through Clauseo* (`SignOut`) keeps the record, so a different account coming
  back reads as changed. Its verdict is signed out: nothing refused by the identity check (a
  launch meets the agent's own sign-in refusal, `agent.auth_required`, never
  `agent.identity_unknown`), no 30 s timer, and the stat watches for a login file coming back,
  which checks. One status run confirms the file's going.
- *A panic in the setup thread's followers* (debug and test builds only: the workspace's
  `[profile.release]` sets `panic = "abort"`, so in a release build a panic there, or in the
  launch gate, aborts the app, and none of what follows runs; the gate is written not to panic).
  Each panic sets `agent.identity_unknown` on every watched agent and starts the followers again,
  three times at most. After the third restart's panic the identity check is dead: a flag pins
  every agent on the shared login at `agent.identity_unknown` (a changed one keeps
  `signin.account_changed`), so neither the pump nor a record can lift it, the gate refuses
  them all, and one error line says so, until Clauseo restarts. Agents with a key login are not
  refused.
- *If the setup host thread fails to start* (`setup/start.rs`), nothing runs the checks a
  start, a stamp change or a launch asks for, so every login whose check is due stays waiting
  at `agent.identity_checking`: it fails closed.
- *A key login is not checked* (the orchestrator's ruling, 2026-11-06; narrowed by the security
  review's H3). Only when the launch env the launch hook builds (`setup_vars`) carries a
  credential that replaces the shared login: the API key variable the hook sets from Clauseo's
  own key store, or a Claude provider switch (`CLAUDE_CODE_USE_BEDROCK`, `_VERTEX`, `_FOUNDRY`),
  which reads no OAuth file. A swapped file cannot redirect those launches: no status runs,
  nothing is refused or stopped, and the verdict reads "key login, not checked". **An endpoint
  alone is checked:** `ANTHROPIC_BASE_URL`, or a Codex endpoint, which adds no variable, still
  launches on the shared login. Any change of a key, an endpoint or a provider works the
  exemption out again from the launch env, drops a check in flight, and, when the exemption
  ends, sets `agent.identity_checking` before `restart_for_login` relaunches.
- *The state table.* Watched: a change of a login file's stamps checks it. Timer: checked
  again by itself. A pending sign-in through Clauseo is the exception to every row below.

  | State | Watched | Timer | Refuses |
  |---|---|---|---|
  | Idle, an account recorded (signed out outside Clauseo) | yes | macOS Claude, 10 min | no |
  | Idle, nothing recorded (not signed in) | yes | no | no |
  | Signed out through Clauseo (account still recorded) | yes | macOS Claude, 10 min | no (the agent's own `agent.auth_required`) |
  | Same | yes | macOS Claude, 10 min | no |
  | Unknown | yes | every 30 s after a failed or timed-out status, else no | `agent.identity_unknown` |
  | Due | yes | no (a launch or the startup asks) | `agent.identity_checking` |
  | Checking | at the next look, against the stamps before its read | no | `agent.identity_checking` |
  | Changed | no | no | `signin.account_changed`, until a sign-in records |
  | KeyLogin | no | no | no |
  | First run (start, nothing recorded) | as Due | no | `agent.identity_checking`, until its startup check |
  | Clauseo sign-in after a launch | as Due until its record | no (a no-email one past its bound: unknown, and a check) | `agent.identity_checking` until the record lands: its email, or the first account its check reads (by id), is recorded even after a launch; no other path records after one |
  | Pending sign-in through Clauseo | by stat: a change refuses until the record lands; no check | no (its bound passing: unknown, and a check) | as its state, or `agent.identity_checking` once its files moved |
  | Dead (followers panicked past three restarts; debug and test builds) | no | no | `agent.identity_unknown`, every agent on the shared login |

- *At start.* Every agent waits (`agent.identity_checking`) until its startup check answers; a
  record that cannot be read counts as unknown for every agent with a login (refused,
  retryable), never as the same, until a sign-in through Clauseo writes it again. Logs name the
  agent and a fixed kind, never an email, a hash or a path.
- *The record's hash.* Plain SHA-256 of the email or id, no salt: a dictionary of emails would
  reverse it, which is acceptable on a single-user machine, where Claude's own config already
  holds the email in plain text. `identity.json` goes into no diagnostics bundle. A missing
  `identity.json` reads as a first run. Sessions will be denied writes to `<data_dir>/agents/`
  once the sandbox's enforcement lands (S14/S15, 0051 PR-D/E); today it is not enforced at
  launch (`clauseo_sandbox`'s `detect.rs` reports `NotBuilt`), so see the known gap below.
- **Known gap (R17).** The code is public, so assume an attacker knows the check.
  - *Token-only swap.* Codex's `account/read` reads the email from the stored `id_token`
    without contacting the server. Swapping the access and refresh tokens while keeping the
    `id_token` passes the check until Codex's next refresh, and the attacker controls when that
    is: the file's `last_refresh` decides it, so it can be put off by days. Claude is presumed
    open: its status email likely comes from `.claude.json`, which a swap doesn't touch.
  - *The data folder, until the sandbox is enforced.* Until S14/S15's enforcement lands (0051
    PR-D/E), a session could delete or rewrite `identity.json`. During a run the in-memory
    record still catches a swap. Across a restart, the startup check would trust a swapped
    account: a missing record reads as a first run, and a rewritten one as the record. No cheap
    marker shows an agent was signed in on the shared login before (`key-stored.json` marks
    stored keys, which are not checked), so nothing tells a deleted record from a first run.
  - *Windows `ChangeTime`.* It can be set through `SetFileInformationByHandle`, so a write
    with all its times restored, and the same length, can go unseen on Windows.
  - *The window of a sign-in.* The record trusts the account the sign-in's confirming status
    read names. Launches are refused once the login files move during the sign-in, so no new
    session starts on a written login; what remains is a write by a session already running (or
    anything else) between the stamps taken at the sign-in's last running state and that read:
    it is recorded as the account signed in.
  - *Closing them.* Both of the first two close with a forced refresh on the check
    (`account/read` with `refreshToken: true`), once stage B's proof shows running sessions in
    two matters survive it without `refresh_token_reused`: a row on the release checklist. For
    Claude, PR-D's read-only `CLAUDE_CODE_OAUTH_TOKEN` closes it where it applies.

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
