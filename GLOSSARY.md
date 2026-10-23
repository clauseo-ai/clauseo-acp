# Glossary

One name per thing. Code, docs and UI use these words exactly. "Avoid" lists words not to use for
the same thing. Lawyer-facing words are marked (UI).

| Term | Meaning | Avoid |
|---|---|---|
| Matter | A lawyer's folder of documents for one piece of work, plus Clauseo's version history of it. | project, workspace, case (in code) |
| Scratch matter | The built-in matter for quick questions with no folder of their own. | default matter |
| Thread | One conversation with one agent inside a matter. | chat, conversation, session (in UI) |
| Turn | One prompt from the lawyer and everything the agent does until it stops. | run, exchange |
| Agent | The lawyer's coding agent (Claude Code, Codex) that Clauseo runs. | assistant (in code), model, bot |
| Adapter | The ACP program that wraps an agent (`claude-agent-acp`, `codex-acp`). | bridge, plugin |
| ACP | Agent Client Protocol, spoken over stdio between Clauseo and an adapter. | |
| Session | An ACP session, the adapter's view of a thread. Not shown to lawyers. | |
| Host | The Rust session host: one thread that owns sessions, commands, permissions and subscriptions. | backend, server, core (for the host alone) |
| Core crates | `protocol`, `reducer`, `store`, `matters`, `host`, `acp`, `sandbox` (with the egress proxy). | |
| Event | One entry in the append-only log, with `event_id`, `stream`, `seq` and kind. | message, record |
| Stream | The log of one thread or one matter (`thread:<uuid>`, `matter:<uuid>`); `seq` and cursors count per stream. | channel, topic, feed |
| Domain event | An event the log stores (`DomainEvent`): what happened in a thread or matter. | host event, notification |
| Host event | What a subscriber receives (`HostEvent`): patches, events, caught up, account limits. Never logged. | domain event, push |
| `seq` | Gapless per-stream number the host assigns to each event. | index, offset |
| Epoch | One load of a stream's view in the host (`ViewEpoch`); cursors count within it, and a subscriber naming another epoch gets a `Reset`. | generation, session (for this) |
| Cursor | Per-stream number on view patches; the webview applies only the next one. | version (for this) |
| View patch | `Upsert`, `Remove`, `AppendText` or `Reset`, sent from host to UI. | diff, delta |
| Error code | The stable dotted string a failed host call carries (`store.disk_full`); the UI picks copy by it. | error message, status |
| Queued prompt | A prompt sent while a turn runs in its thread; it waits as its own turn and can be edited, removed or sent now. | pending message, draft |
| Send now | Cancel the running turn and send this prompt next. | interrupt, force send |
| Steer | Add a prompt to the running turn where the agent can, else send it now. A real steer ends as `SteerOutcome::Injected`; one that came as the turn ended ends as `Requeued`, and one sent as send now (the agent cannot steer) as `SentNow`. | inject (in prose), nudge |
| Queue held | A thread's queued turns wait until the lawyer resumes them (`HostCommand::ResumeQueue`); a cancel, a stop at the token limit or a restart holds the queue (`HeaderEntry::queue_held`). | paused |
| Subscriber | Anything that receives view patches for a stream (a window now, a phone later). | listener |
| Checkpoint | A commit in the matter's shadow repository, taken before and after each turn. | snapshot (taken for store snapshots) |
| Blob | A stored payload too large to keep inline, named by its BLAKE3 hash; a `TextOrBlob` holds it as a `BlobRef` with a preview. | attachment, file (for this) |
| Snapshot | A stored fold of a stream's events (a thread's or a matter's), so reopening skips the full log. | checkpoint |
| Versions (UI) | What lawyers see of checkpoints. | git, commits, history (in UI) |
| Step (UI) | An ended turn as the lawyer sees it: its step card, its changes and its undo. The code names its id `turn` (`StepEntry`, `HostCommand::UndoStep { turn }`, `DomainEvent::StepUndone { turn }`, `MarkerKind::StepUndone`). | turn (in UI) |
| Changes in this step (UI) | The files one turn changed. | diff (in UI) |
| Undo (UI) | Move the matter forward to a state that reverses a step. Never rewrites history. | revert, rollback |
| Marker | A line in a thread or matter view recording an undo, a restore or a compaction (`MarkerEntry`, `MarkerKind`). | divider, banner |
| Outside edits | Files changed in the matter folder outside Clauseo between turns (the lawyer, Word, a sync client), captured in a checkpoint and logged on the matter stream (`DomainEvent::OutsideEdits`). | external changes |
| Restore (UI) | Bring back a file or the matter as it was at a version. | checkout |
| Compare (UI) | Show the difference between two versions. | diff (in UI) |
| Sandbox | The OS-level jail around the agent process. | VM, container |
| Tier A, B, C | Protection levels: A needs no admin, B uses a sandbox user (Windows, one admin prompt), C has no sandbox. | |
| Not protected (UI) | What a matter shows when it runs in tier C. | unsafe, insecure |
| Auto mode | The adapter preset that acts without asking; on by default only where the sandbox is active. | YOLO, full access |
| Egress proxy | Clauseo's local CONNECT-only proxy, the agent's only route to the network. | firewall |
| Permission request | The agent asking before a tool action, logged as `PermissionRequested`; any client may answer, the first valid answer wins, and it can expire unanswered. | approval prompt, confirmation |
| Elicitation | The agent asking the lawyer for input, as a form Clauseo renders or a page opened outside the app (`ElicitationRequest`); the answer's values are not logged. | input request, question |
| Notice | A message for the lawyer from the agent or the host, with a level (`Notice`, `NoticeEntry`). | toast, alert |
| Denial | A blocked path or domain, reported to the host as an event and shown as a card. | error |
| Launcher | The trait `process` uses to start an agent: passthrough or one of the sandbox launchers. | runner, spawner |
| Policy layer | Settings pushed by a firm's IT (registry, managed preferences, policy file). | admin config |
| Profile | Clauseo's instructions, tools, helper agents and skills, translated per agent. | persona, prompt |
| Skill | A packaged instruction set in Agent Skills format (`SKILL.md`). | recipe, macro |
| Workspace | Only Anthropic's API workspace, as named in limit reasons (`LimitStopReason::WorkspaceCredits`, `WorkspaceSpendCap`); those logged names stay. Never a matter. | |
| Principal | Who made a call; only `LocalDesktop` today. | user (for this) |
| Blank app | The empty Tauri shell, kept as the control for memory and start-up budgets. | hello world |
| Lite mode | Reduced rendering for remote sessions, software rendering or reduced motion. | low-power mode |
