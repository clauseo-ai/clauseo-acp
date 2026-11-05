# Provenance of code derived from Codex

Tier B's ACL code follows Codex's Windows sandbox. Upstream: https://github.com/openai/codex, tag
`rust-v0.162.1`, commit `092d3acd6bec3e3a14bdc7e7a2810ab628ab759d`, licence Apache-2.0 (text and
upstream NOTICE in `LICENSES/Apache-2.0-codex.txt`). Clauseo is GPL-3.0-or-later; the files below
carry both identifiers. A follow-up checks upstream each quarter.

| File here | Upstream | What came from there | What changed |
|---|---|---|---|
| `acl.rs` | `codex-rs/windows-sandbox-rs/src/acl.rs` | One open per target, its descriptor read and written through that handle (`fetch_dacl_handle`, `add_deny_ace`); no rewrite when the entry is there; a whole-tree deny refused on a volume's root (`ensure_handle_is_not_filesystem_root`) | Rewritten on the `windows` crate. The open never follows a link (`FILE_FLAG_OPEN_REPARSE_POINT`) and checks the final path; `SetKernelObjectSecurity` instead of `SetSecurityInfo`, with Clauseo's own walk (D7 skips, strict refusals, labels); the Low label; no `anyhow`, no paths in errors |
| `acl_bytes.rs` | `codex-rs/windows-sandbox-rs/src/acl.rs` | The ACE scans (`dacl_has_deny_mask`, `dacl_mask_allows_with_scope`: skip inherit-only and inherited entries as the check needs); the reason an allow can beat another trustee's inherited deny | Over bytes instead of `GetAce`; entries added and removed exactly instead of through `SetEntriesInAclW`, which merges an allow into the trustee's existing one |

Nothing else under `windows/` is derived from Codex. User creation, DPAPI and WFP (PR4, PR7) add
their rows when they land.
