// UC's test fixtures: a matter's protection in each state, and a computer's ProtectionView. Only tests import it.
import type { Protection, ProtectionView } from "../../transport/protocol.ts";

export const np = (cause: unknown): Protection => ({ not_protected: { cause } }) as Protection;
export const PROT = {
  a: { protected: { tier: "a" } } as Protection,
  b: { protected: { tier: "b" } } as Protection,
  needs: np("needs_set_up"),
  refused: np("set_up_refused"),
  failed: np("set_up_failed"),
  unsupported: np("unsupported"),
  newer: np("some_newer_cause"),
  pool: np({ pool_full: { size: 8, open: 5, held: 0, owed: 2, set_aside: 1 } }),
  off: np({ switched_off: { at: 1_762_400_000_000 } }),
  // design B: R17 PR-E's causes, not in bindings.ts yet
  missing: np({ blocked: { reason: "admin_copy_missing" } }),
  logon: np({ blocked: { reason: "logon_right" } }),
  policy: np({ blocked: { reason: "policy" } }),
  blockedNewer: np({ blocked: { reason: "a_reason_from_a_newer_clauseo" } }),
  otherAccount: np("other_account"),
};

/** A view whose set-up brings no Windows prompt (R17's retry_asks_admin false), so the app offers
 * it. */
export const NO_PROMPT = { retry_asks_admin: false } as Partial<ProtectionView>;

export const view = (computer: Protection, over: Partial<ProtectionView> = {}): ProtectionView => ({
  tier: "protected" in computer ? (computer.protected?.tier ?? "c") : "c",
  computer,
  required: false,
  unprotected_auto_allowed: true,
  ...over,
});
