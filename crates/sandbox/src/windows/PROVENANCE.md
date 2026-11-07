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
| `wfp/engine.rs` | `codex-rs/windows-sandbox-rs/src/wfp.rs` | An engine handle closed on drop; a transaction aborted unless committed; the provider and sublayer added with `FWP_E_ALREADY_EXISTS` taken as there; each persistent filter deleted by key, then added; an `ALE_USER_ID` condition over a security descriptor granting `FWP_ACTRL_MATCH_FILTER` | Rewritten on the `windows` crate; the blocks' descriptor denies the pool group and every pool user, matched `NOT_EQUAL`; a static permit per pool user; weights, a block that clears its action right, a soft permit; the conditions (`engine/conditions.rs`), the owner's read-only grants (`engine/security.rs`) and the reads and enumeration a launch checks (`engine/read.rs`) are Clauseo's own; no `anyhow` |
| `wfp/spec.rs`, `wfp/rules.rs` | `codex-rs/windows-sandbox-rs/src/wfp/filter_specs.rs`, `setup_provisioning/firewall.rs` | The filters as a list of plain specs with fixed keys; ICMP blocked at resource assignment; the firewall's loopback and non-loopback address literals | Clauseo's own filters: every connect blocked, loopback and the range included, inbound outside loopback blocked; one static permit per pool user for the range; the firewall's range exception is 61080-61179 |
| `wfp/firewall.rs` | `codex-rs/windows-sandbox-rs/src/setup_provisioning/firewall.rs` | COM initialised and balanced per call; each rule found by name or made, its fields written before it is added and again on every run; `LocalPolicyModifyState` read through the vtable so `S_FALSE` shows; the rule's `LocalUserAuthorizedList` read back | A firewall that Group Policy ignores, or a rule that can't be written, is reported and never fails the set-up (WFP enforces alone); the rules name every pool user; no logging to a file; no `anyhow` |

For the S15 PR3 review fixes, upstream was read again at commit
`e7f32cf1928a35a9232a633a51c2a03b5a7e2c72` (`codex-rs/windows-sandbox-rs`, Apache-2.0). Taken from
there: `OBJ_DONT_REPARSE` on the walk's handle-relative opens (`acl/object.rs`, after
`no_reparse_dir.rs`'s `open_no_reparse`). Its `acl.rs` was compared and nothing more taken: its
allow scans read plain allows only (`ACCESS_ALLOWED_ACE_TYPE`), where the strict walk refuses an
object or compound allow of any SID, and it edits no mandatory label.

Nothing else under `windows/` is derived from Codex. The WFP rows (S15 PR7) were read at commit
`092d3acd6bec3e3a14bdc7e7a2810ab628ab759d`. S15 PR4e's sign-in screen hiding (`setup/accounts.rs`)
and set-up mutex (`setup/lock.rs`) follow the approach of Codex's `hide_users.rs` and
`setup_mutex.rs`, and take no code from them.
