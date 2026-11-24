// Whether a conversation met Clauseo Case Law's out-of-credit refusal (orchestrator r7458, r7462): it holds the host's
// case_law_out_of_credit notice after the lawyer's last message, read from the conversation's own entries, so it holds
// after a reload, in a second window and for a conversation that isn't open, and the lawyer's next message ends it by
// itself. The composer's "Credits added. Ask again to search case law." shows when the credit is back and this holds.
// The pay link comes only from the connectors' status, never from here.
import type { ViewEntry } from "../../transport/protocol.ts";
import { isCaseLawOutOfCredit } from "../thread/case-law-notice.ts";

/** Whether the out-of-credit notice comes after the lawyer's last message, in order. */
export function metRefusal(entries: readonly (ViewEntry | undefined)[]): boolean {
  for (let i = entries.length - 1; i >= 0; i--) {
    const entry = entries[i];
    if (entry?.user_message !== undefined) return false;
    if (isCaseLawOutOfCredit(entry?.notice?.kind)) return true;
  }
  return false;
}
