# Threat model

A stub. It says what Clauseo protects, where the trust boundaries are, and what stops an attacker
at each one. Each control has a status: `built` means code and a check exist on `main`; `designed`
means the design is decided and the code is not written yet. The record of the Rust-to-webview
boundary is [0001](decisions/0001-rust-webview-boundary.md).

## Assets

- **The lawyer's documents.** Every file in every matter folder, and the threads about them.
- **Agent credentials.** The sign-in tokens Claude Code and Codex keep for the lawyer. Clauseo
  never reads them.
- **Clauseo's version store.** The checkpoints of each matter. Undo and restore depend on it, so
  an agent must not be able to rewrite it.

## Trust boundaries

1. **Webview to host.** The React UI runs in the system webview and may render text an attacker
   wrote (agent output, document contents). Everything it sends to Rust is untrusted.
2. **Agent to host.** The agent and its adapter run as a separate process and speak ACP over
   stdio. A prompt injection in a document can steer the agent, so its messages are untrusted.
3. **Agent to the machine.** The sandbox stands between the agent process and the lawyer's other
   files.
4. **Agent to the network.** The egress proxy is the agent's only route out.

## Webview

**What an attacker controls.** Script in the page, through an XSS in rendered agent output or in a
converted document. From there they can call any IPC command the page can reach.

**What stops them.**

- `built`: Capability files grant nothing; the `capabilities` check fails on `remote`,
  `core:default`, `fs:`, `shell:`, `http:`, `opener:` or `dialog:`
- `built`: A pinned CSP: no remote images or connections, no `unsafe-eval`, `object-src 'none'`,
  `base-uri 'none'`, Trusted Types; the `tauri-config` check pins the string
- `built`: An `AppManifest` in `build.rs`, so app commands go through the access check; the command
  list is empty
- `built`: No global Tauri object, a frozen prototype, no asset protocol (`tauri-config` check); no
  devtools (`plugins` check and `deny.toml`)
- `designed`: Rust validates every command (types, opaque ids, allowed values); files are named by
  opaque ids, never paths
- `designed`: A minimal command surface: send, subscribe, acknowledge, answer a permission, pick a
  folder, open a file or link
- `designed`: Escalations only through native dialogs drawn by Rust, never a webview confirm
- `designed`: The webview never navigates; links go through one `open_external` (https, http,
  mailto) with a native confirm; `open_file` opens only document types inside a matter
- `designed`: Markdown renders without raw HTML; DOMPurify only on the few HTML-string paths

## Agents

**What an attacker controls.** The agent's behaviour, through a prompt injection in a document,
a web page or a tool result. The agent can read and write whatever the sandbox allows, and send
any ACP message, including permission requests with misleading text.

**What stops them.**

- `designed`: The sandbox wraps the whole agent process (adapter and agent), not only its shell
  commands
- `designed`: Auto mode is on by default only where the sandbox is active; in tier C the adapters'
  ask modes apply and the matter shows Not protected
- `designed`: A human approves only escalations: paths outside the matter, new web domains, sandbox
  off, full-access modes, "always allow", all through native dialogs or denial cards
- `designed`: Permission answers are checked against the options offered; options are labelled by
  kind, not by the agent's text
- `designed`: ACP message sizes are capped; Clauseo advertises no file or terminal access to the
  adapter
- `designed`: The host checkpoints the matter before and after every turn, so any step can be undone
- `designed`: Adapters are installed pinned by version and integrity hash

## Sandbox

**What an attacker controls.** Code running as the agent process: shell commands, file tools and
anything the agent starts.

**What stops them.**

- `designed`: An OS jail per tier: tier A is Seatbelt (macOS), bubblewrap (Linux) or MXC (Windows);
  tier B is a sandbox user on Windows; tier C has no sandbox
- `designed`: The agent reads and writes only the matter folder and a per-matter agent state folder;
  it reads its own runtime, read-only
- `designed`: Its credential file is linked into the state folder, never copied or read by Clauseo;
  sign-in runs outside the sandbox
- `designed`: Any `.git` already in the folder and any file over the size cap are read-only to the
  agent
- `designed`: Clauseo's version store is outside the matter folder and invisible to the agent
- `designed`: A blocked path becomes a denial event and a card; widening access restarts the agent
  only if the lawyer accepts

## Egress proxy

**What an attacker controls.** Network requests from inside the sandbox, aimed at sending the
lawyer's documents somewhere.

**What stops them.**

- `designed`: The sandbox allows no network except Clauseo's local proxy; raw sockets are refused
- `designed`: The proxy is CONNECT-only, with an allowlist of domains measured per agent
- `designed`: It never intercepts TLS; upstream it follows the OS proxy settings
- `designed`: A blocked domain becomes a denial event and an "Allow this site?" card
