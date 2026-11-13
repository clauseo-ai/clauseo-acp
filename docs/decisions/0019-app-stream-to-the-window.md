# 0019: The app stream to the window, and allow-lists for `dispatch` and `query`

## Context

[0017](0017-settings-over-the-wire.md) gave the host display settings over the wire
(`GetSettings`, `SetUiSettings`) and app-wide events (`HostHandle::subscribe_app`, today only
`SettingsChanged`). The room needs them in the real app: it reads and changes the settings and
follows every change, whoever made it. Later host rows add more reads and commands the same way:
H8's `list_papers` (#131), H3's `relocate_matter` (#135), H4's matter list and thread summaries on
the app stream.

The reads and commands already reach the host. [0012](0012-desktop-bridge.md)'s `query` and
`dispatch` take protocol's own `HostQuery` and `CommandEnvelope`, which decode and validate in
Rust, and run as `LocalDesktop`. Only the app stream had no path to the page. 0012 asks for a new
record for a tenth command.

There were two ways to carry the new calls. One typed command per call means four commands now,
two more for #131 and #135, and one more per host row after that, each with its own permission,
list lines and binding. Each would repeat a variant of an enum that Rust already checks, and the
security would be no better. The other way is the checked passthrough that exists. Its weakness
was `dispatch`'s deny-list: it refused three commands and let any other through, so a command the
host added later reached the page without anyone deciding it should.

## Decision

- **Reads and commands stay on `query` and `dispatch`.** Their request types stay the host's own
  enums, decoded and validated in Rust.
- **Both are allow-lists.** `Bridge::admit` matches every `HostCommand` variant by name, with no
  catch-all, and `query` matches every `HostQuery` variant the same way. A variant the host adds
  does not compile in `crates/desktop` until the bridge rules on it.
  - `admit` refuses `AnswerPermission`, `SetMode`, `SetConfigOption`, and a `CreateMatter` whose
    folder ref this run's `pick_folder` did not issue, as before, all with `command.invalid`.
  - H3's `RelocateMatter` (#135) takes only a folder ref this run's `pick_folder` issued, the
    same check, refused with `command.invalid` before the host. The rest of H3 (the
    `matter_relocated` event and `MatterSummary.folder_state`) rides the existing streams and
    reads and needs no rule here.
  - H8's `ListPapers` (#131) is a read and needs no rule: `query` lets it through.
  - Slots wait as comments for H5 and H20's `UpdateMatter`, `OpenMatter`
    and `ForgetMatter`, and H20's `GetDefaultLocation`. H20's `CreateMatter` source `default_location` carries no folder ref and
    passes; the folder-ref check stays on `new_folder` and `existing_folder`.
- **One new command, `subscribe_app()`.** It takes no arguments, and it needs `open_stream` first
  (otherwise it fails with `command.invalid`).
  - It subscribes the window's existing channel to `subscribe_app(LocalDesktop)`. Each message is
    the host's encoded `HostEvent` (`{"settings_changed": SettingsView}`), framed and counted like
    a stream's (0012's 8 KB messages, the 256 KB cap, the watchdog). It carries no position, so
    an `ack` names none for it.
  - It starts with nothing. The page subscribes, then reads `GetSettings`. The host answers in
    order, so no change falls between the two (0017).
  - Calling it again restarts it. A new `open_stream` (a reload) drops it with the window's other
    subscriptions.
  - A pause or a watchdog fire drops it with the streams. The restart that follows subscribes
    again, reads `GetSettings`, and sends one `settings_changed` before any event of the new
    subscription. So the window may miss steps but never ends on a stale view.
  - The host refuses `subscribe_app` only once it has stopped, so a failed restart is not
    retried: the window stops following the app stream.
- **The capability.** `allow-subscribe-app` is granted by `capabilities/main.json` only. It is in
  `build.rs`'s `RELEASE` and `commands.rs`'s `COMMANDS`, and the test that compares the lists
  (`every_list_of_commands_agrees`) and the bindings drift test cover it.
- **The room.** In the real app, `nextHostOf` (`ui/app/src/room/host-next.ts`) gives the room the
  settings part of `NextHost`, built on `query`, `dispatch` and the app stream. Its other parts
  stay empty until the host has their rows. Tests, the probe and the copy review keep the stand-in.

## Consequences

- The page reaches ten typed commands. The threat model gains two things:
  - Every host command and read the page can send is named in Rust. A new one fails closed until
    a change to this crate admits it.
  - The new command is read-only: it takes nothing from the page and only adds messages to the
    window's own channel, under the same byte cap.
- A host row that adds a read or a command needs one arm in `admit` or `query` and a test. It
  needs no new permission and no new command, unless its arguments need a check that decoding
  cannot do (a folder ref, a native confirm). In that case the check goes in `admit`.
- Every app event reaches every window that subscribed. A window drops what it does not need
  (0017).
- The room is unreleased, so the display values it kept in localStorage before this change are
  not carried over. With a host, localStorage becomes a cache of the host's last answer, read
  only for the next start's first frame; the host's answer then replaces it. A start shows the
  wrong settings for a moment only when they changed while the room was not running.
- R12's internal window sends the same calls as before. A bridge test,
  `every_command_and_read_the_windows_send_today_is_admitted`, lists them and fails if one is
  refused.

## Amendment 2026-11-10: the window's network signal (D-050)

- **One more host command, `HostCommand::SetNetwork { online }`,** on `dispatch` as the rule above
  says: one arm in `admit_main`, `main` only, and the bridge test lists it. No new Tauri command
  and no new permission.
- **What it carries:** one flag from the page, the webview's `navigator.onLine`. The main window
  sends it once when it mounts (the host outlives a reload), at each `online`/`offline` event, and
  before a `send_prompt` when `navigator.onLine` changed unseen, but only when the value differs
  from the last one sent (`ui/app/src/transport/network.ts`). It answers `Done` and stores no
  receipt or event.
- **What it can do:** while it says offline, a running turn whose agent then sends nothing for
  30 s is checked by the host itself: one TCP connection, within 3 s, to the agent's service or
  the proxy the egress route names (`offline/probe.rs`). Only if that fails too does the turn end
  `Failed { agent.offline }` through the Cancel path, the answer so far kept
  (`crates/host/src/actor/turn/offline.rs`); if it gets through, the watch waits another 30 s.
  Any agent activity, or `online`, takes the watch back. A turn waiting on the lawyer (a
  permission, an elicitation) is never ended, and one running a tool call is not for 10 minutes
  from the watch's start; after that the probe decides. The window's word alone never makes a
  failed prompt read as offline. So a page that lies can stop only a turn whose agent is
  already silent and whose service the host cannot reach either.
- **The same probe guards an agent's own note** (Codex's "Falling back from WebSockets to HTTPS
  transport", Claude's connection line): after it, 30 s of silence ends the turn only when the
  probe fails, since Codex then retries over HTTPS, which a blip or a proxy that blocks only
  WebSockets lets through. A tool call open after such a note does not extend the wait.
- **Codex's typed failures arm it too (2026-11-11, stage C):** to a client that advertises notices
  and the AIR `sessionFailure`, as Clauseo does, codex-acp 2.2.2 sends each of Codex's retries as
  a `session_info_update` whose `sessionFailure` has category `connection` (its `transport_lost`),
  Codex's fallback warning as a `notice`, and the end of its retries as the prompt's `end_turn`
  with a `connection` failure of severity `error`. Codex 0.162 retries a failed connection without
  end, so only the watch ends such a turn. A `connection` failure arms the watch like the note,
  and the wait runs from the first note: later retries only replace it, so retries less than
  30 s apart never put the probe off. The notice is the note and never reaches the transcript,
  in a turn or outside one (idle, or replayed by `session/load`); answer text outside a turn is
  never touched. A prompt that ends on a failure Codex gave up on ends `agent.offline` when a
  note armed the watch or its title names a connection failure, unless its title says a server
  answered or the host's last probe got through (an outage of the service, not of the
  computer); else it fails `agent.protocol_error`, as the same failure over JSON-RPC does, so a
  cut-off answer never shows as complete.
- **A blocked upstream behind a live proxy (2026-11-13, D-050 case 2):** stage C's proxy answered
  every `CONNECT` with 502; Codex's fallback note named the status (not offline), its retries
  came about a minute apart without end, and the probe, which only TCP-connected to the live
  proxy, always got through, so the turn said "Answering." for good. Four changes
  (`offline/probe.rs`, `offline/probe/route.rs`, `offline/quiet.rs`):
  - **The probe asks the service, not the proxy, where the agent goes** (security review and
    stage D review, round 1). At each launch the host records on the agent what the probe asks,
    from the launch's final environment, the one the agent gets (`offline/probe/service.rs`),
    so no route is looked up when it runs and the 8 s cover only the name's lookup, TCP,
    `CONNECT` and the hello:
    - *the service:* a base URL that environment sets (`ANTHROPIC_BASE_URL`, a firm plan's
      endpoint included; `OPENAI_BASE_URL`); else Codex's `api.openai.com` when its launch
      passes a key, `chatgpt.com` for Clauseo's own login (the key retry's `Own` rule), and
      both for the lawyer's reused login, whose kind the host never reads (either answering
      counts); Claude's `api.anthropic.com`. A Claude on Bedrock, Vertex or Foundry
      (`CLAUDE_CODE_USE_*`) or a base URL that names no one `http`/`https` host is
      `unknown_service`;
    - *the route:* the proxy that environment hands the agent, with its `NO_PROXY`, through the
      sandbox's `launch_route` (the egress proxy's own reading of a proxy URL and `NO_PROXY`).
      Desktop's launch puts the route it resolved there (a firm's `network.upstream_proxy`
      first, then `HTTPS_PROXY`/`ALL_PROXY`, then the OS's, `setup/network.rs`), and what a
      plan, the agent's entry or the inherited environment set stays as the agent reads it:
      Claude Code `https_proxy`, `HTTPS_PROXY`, `http_proxy`, `HTTP_PROXY` and both
      `no_proxy` lists; Codex `HTTPS_PROXY`, `https_proxy`, `ALL_PROXY`, `all_proxy` and
      `NO_PROXY`. A proxy a `CONNECT` cannot follow (`https://`, `socks5://`) is
      `unknown_route`.
    Either unknown is not probed: it counts as reached at once and its line says so, so a watch
    waits and only the backstop (or the agent's own end) ends a silent turn. Through a proxy it
    sends `CONNECT`
    with no credentials: 200 goes on, 407 counts as reached, any other status or no head within
    the cap is a miss. Then a TLS client hello (straight to the host when there is no proxy):
    a handshake record back is reached; an alert, other bytes, a close or silence is a miss.
    No certificate is checked and nothing but the hello is sent, so a firm's TLS-inspecting proxy
    counts as reached; a hello that cannot be written is a miss (`tls_other`). The whole try
    has 8 s (a reused Codex login's two hosts side by side). A plain `http` base URL gets the
    TCP connection only.
  - **The quiet arm.** Every live turn of Claude or Codex keeps the time of its last model output
    (answer and thought chunks, tool calls and their updates, plans, compaction updates, an ask),
    whether or not its service is known; an ask's end restarts it, so the lawyer's time never
    counts. An agent of no known kind gets no quiet arm. 30 s with no output, no open tool call
    and no ask arms a quiet watch and probes at once; a probe that gets through waits again, and two
    misses in a row end the turn `agent.offline`. An agent's note turns the watch into a note's,
    which ends at its next miss; the window's `offline` makes it the window's, and `online`
    leaves it. This is what covers Claude, whose retries reach the client as nothing at all.
  - **The backstop:** 5 minutes with no output, no open tool call and no ask end the turn
    `Failed { agent.quiet }` ("… stopped answering. Try again.", the answer so far kept, Try
    again, the queue held), whatever the probes said; a reached probe never moves it.
  - **An open tool call** keeps the quiet arm and the backstop off only while the agent sent
    something (a tool update counts) within `TOOL_LIMIT` (11 min, stage D round 2: a minute
    past Claude Bash's own 600 s timeout, so a command that times out at 10:00 ends with
    Claude's report, not `agent.quiet` a moment before); past it the silence counts, so a tool
    left open with no word for 11 min ends the turn `agent.quiet` then. A window-only watch
    keeps its own `WORKING_LIMIT` (10 min).
  - **Gaps the backstop is what ends:** a proxy that asks for credentials (407, counted as reached
    so a proxy login never reads as offline) and a TLS-inspecting proxy that answers the
    handshake itself, both in front of a blocked upstream; a Claude on a cloud provider's login,
    or behind an `https://` or SOCKS proxy (not probed); and a Claude compaction whose summary
    sends nothing for 5 minutes after its start (claude-agent-acp 0.89.1 sends
    `compaction_update` at the start and streams `compaction_summary_chunk`, each counted as
    output, but nothing comes between them while the summary call waits for its first token).
  - **The probe's line reaches host.log**, at the lifecycle target, with fixed fields only:
    `reached`, `noted`, `by` (note, window, quiet), `class`, `via`, `status`, `ms`, `misses`;
    never a host, a proxy or bytes. Stage C's case 1 (proxy killed) had the probe run unseen:
    host.log keeps INFO from the lifecycle targets only.

## Status

proposed

## Date

2026-11-02
