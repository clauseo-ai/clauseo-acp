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
| Event | One entry in the append-only log, with `event_id`, `thread_id`, `seq` and `kind`. | message, record |
| `seq` | Gapless per-thread number the host assigns to each event. | index, offset |
| Cursor | Per-thread number on view patches; the webview applies only the next one. | version (for this) |
| View patch | `Upsert`, `Remove`, `AppendText` or `Reset`, sent from host to UI. | diff, delta |
| Subscriber | Anything that receives view patches for a thread (a window now, a phone later). | listener |
| Checkpoint | A commit in the matter's shadow repository, taken before and after each turn. | snapshot (taken for store snapshots) |
| Snapshot | A stored fold of a thread's events, so reopening skips the full log. | checkpoint |
| Versions (UI) | What lawyers see of checkpoints. | git, commits, history (in UI) |
| Changes in this step (UI) | The files one turn changed. | diff (in UI) |
| Undo (UI) | Move the matter forward to a state that reverses a step. Never rewrites history. | revert, rollback |
| Restore (UI) | Bring back a file or the matter as it was at a version. | checkout |
| Compare (UI) | Show the difference between two versions. | diff (in UI) |
| Sandbox | The OS-level jail around the agent process. | VM, container |
| Tier A, B, C | Protection levels: A needs no admin, B uses a sandbox user (Windows, one admin prompt), C has no sandbox. | |
| Not protected (UI) | What a matter shows when it runs in tier C. | unsafe, insecure |
| Auto mode | The adapter preset that acts without asking; on by default only where the sandbox is active. | YOLO, full access |
| Egress proxy | Clauseo's local CONNECT-only proxy, the agent's only route to the network. | firewall |
| Denial | A blocked path or domain, reported to the host as an event and shown as a card. | error |
| Launcher | The trait `process` uses to start an agent: passthrough or one of the sandbox launchers. | runner, spawner |
| Policy layer | Settings pushed by a firm's IT (registry, managed preferences, policy file). | admin config |
| Profile | Clauseo's instructions, tools, helper agents and skills, translated per agent. | persona, prompt |
| Skill | A packaged instruction set in Agent Skills format (`SKILL.md`). | recipe, macro |
| Principal | Who made a call; only `LocalDesktop` today. | user (for this) |
| Blank app | The empty Tauri shell, kept as the control for memory and start-up budgets. | hello world |
| Lite mode | Reduced rendering for remote sessions, software rendering or reduced motion. | low-power mode |
