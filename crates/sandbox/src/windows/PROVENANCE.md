# Provenance of code derived from Codex

Tier B's ACL code follows Codex's Windows sandbox. Upstream: https://github.com/openai/codex, tag
`rust-v0.162.1`, commit `092d3acd6bec3e3a14bdc7e7a2810ab628ab759d`, licence Apache-2.0 (text and
upstream NOTICE in `LICENSES/Apache-2.0-codex.txt`). Clauseo is GPL-3.0-or-later; the files below
carry both identifiers. A follow-up checks upstream each quarter.

| File here | Upstream | What came from there | What changed |
|---|---|---|---|
| `acl.rs`, `acl/object.rs`, `acl/walk.rs` | `codex-rs/windows-sandbox-rs/src/acl.rs` | One open per target, its descriptor read and written through that handle (`fetch_dacl_handle`, `add_deny_ace`); no rewrite when the entry is there; a whole-tree deny refused on a volume's root (`ensure_handle_is_not_filesystem_root`) | Rewritten on the `windows` crate. The open never follows a link (`FILE_FLAG_OPEN_REPARSE_POINT`) and checks the final path; `SetKernelObjectSecurity` instead of `SetSecurityInfo`, with Clauseo's own walk (D7 skips, strict refusals, labels), which lists each folder through its handle and opens each child relative to it (`NtCreateFile`); a lawyer's own explicit label read and put back; the Low label; no `anyhow`, no paths in errors |
| `secrets/dpapi.rs` | `codex-rs/windows-sandbox-rs/src/dpapi.rs` | The call shape: `CryptProtectData` and `CryptUnprotectData` over a borrowed input blob with `CRYPTPROTECT_UI_FORBIDDEN`, the output copied out and freed with `LocalFree` | User scope for the owner (set-up acts as the owner) instead of `CRYPTPROTECT_LOCAL_MACHINE` (D-B3); a fixed entropy; the output freed by a guard and a plaintext wiped before it is freed; errors carry the call and code only; no `anyhow` |
| `acl_bytes.rs` | `codex-rs/windows-sandbox-rs/src/acl.rs` | The ACE scans (`dacl_has_deny_mask`, `dacl_mask_allows_with_scope`: skip inherit-only and inherited entries as the check needs); the reason an allow can beat another trustee's inherited deny | Over bytes instead of `GetAce`; entries added and removed exactly instead of through `SetEntriesInAclW`, which merges an allow into the trustee's existing one |

For the S15 PR3 review fixes, upstream was read again at commit
`e7f32cf1928a35a9232a633a51c2a03b5a7e2c72` (`codex-rs/windows-sandbox-rs`, Apache-2.0). Taken from
there: `OBJ_DONT_REPARSE` on the walk's handle-relative opens (`acl/object.rs`, after
`no_reparse_dir.rs`'s `open_no_reparse`). Its `acl.rs` was compared and nothing more taken: its
allow scans read plain allows only (`ACCESS_ALLOWED_ACE_TYPE`), where the strict walk refuses an
object or compound allow of any SID, and it edits no mandatory label.

Nothing else under `windows/` is derived from Codex. User creation, DPAPI and WFP (PR4, PR7) add
their rows when they land. S15 PR4e's sign-in screen hiding (`setup/accounts.rs`) and set-up mutex
(`setup/lock.rs`) follow the approach of Codex's `hide_users.rs` and `setup_mutex.rs`, and take no
code from them.
