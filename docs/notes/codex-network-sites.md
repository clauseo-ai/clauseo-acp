# Codex network sites: kept for later

Verdict from a source read of Codex 0.162.1 and codex-acp 2.2.2, 2026-11-16. Nothing was run.
Not built for launch: ChatGPT's commands don't reach the internet, and Codex escalations that
carry a web address keep being refused (`crates/host/src/actor/turn/shell_network.rs`).

## How it could work (about 16 to 24 hours)

- **Config:** at thread start, set `features.network_proxy.enabled = true` and
  `features.network_proxy.domains = {"<site>" = "allow"}` from the matter's `site` entries
  (`docs/decisions/0063-always-for-this-matter.md`). Also set `sandbox_mode = "workspace-write"`
  with `sandbox_workspace_write.network_access = true` (Codex
  `features/src/feature_configs.rs:503-523`, `core/src/config/permissions.rs:137-172`,
  `config/network_config.rs:98-109`).
- **Answers:** a network approval for a stored host is answered `allow_once`, which Codex reads
  as `Approved`: that host, once, nothing written (`core/src/tools/network_approval.rs:894-905`).
  `shell_network.rs` would stop refusing `url` requests for stored hosts and keep refusing
  escalations.

## Risks

1. The proxy is experimental and off by default (`features/src/lib.rs:1387-1394`).
2. The unelevated Windows sandbox drops `proxy_enforced` and likely bypasses the proxy
   (`core/src/exec.rs:714-726`).
3. The domain list is fixed at thread start (`session/mod.rs:1311-1326`). Each new connection,
   and each redirect hop to another host, needs its own answer (`network_approval.rs:316-345`).
4. DNS rebinding is not fully covered (`network-proxy/README.md:266-268`).
5. Unconfirmed whether `features.*` takes effect when passed through codex-acp's
   `CODEX_CONFIG` rather than `app-server -c` (codex-acp `dist/index.js:45657-45677`).
