// What the side says under "Connectors" (U15): where Indian Kanoon stands, in a few words. In the settings window's
// first load with the section's entry, so it holds only the words and the money rule, never the section's body.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { count } from "@clauseo/components/lib/money.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { CASE_LAW } from "../../store/connectors.ts";
import type { ConnectorView, ConnectorsState } from "../../transport/protocol.ts";

/** The connector the section shows: Indian Kanoon's, or the first listed. */
export const shownConnector = (state: ConnectorsState): ConnectorView | undefined =>
  state.connectors.find((c) => c.id === CASE_LAW) ?? state.connectors[0];

/** The side's note: off by the firm or on this computer, else the status; for a balance the credits left, nothing
 * before the first account read. Never Balance.paise: it is the account's figure before GST, and every amount on screen
 * is GST-inclusive (the orchestrator's ruling of 2026-11-06), so rupees wait for stage E's gross rate. */
export function connectorsNote(state: ConnectorsState, lang: Lang): string | undefined {
  const c = shownConnector(state);
  if (c === undefined) return undefined;
  if (!state.available)
    return cw(lang, state.locked ? "Turned off by your firm" : "Turned off on this computer");
  const status = c.status;
  switch (status) {
    case "not_added":
      return cw(lang, "Not added");
    case "signing_in":
      return cw(lang, "Signing in");
    case "signed_out":
      return cw(lang, "Signed out");
    case "key_revoked":
      return cw(lang, "Key no longer works");
  }
  if ("signed_in" in status && status.signed_in !== undefined) {
    const balance = status.signed_in.balance;
    if (balance === undefined) return undefined;
    return cw(lang, "{credits} credits left", { credits: count(balance.credits) });
  }
  if ("out_of_credit" in status && status.out_of_credit !== undefined)
    return cw(lang, "Out of credit");
  // a sign-in that worked but could not be saved says "You're signed in" on the block, so the side says nothing
  if ("error" in status && status.error !== undefined)
    return status.error.kind === "persist_failed" ? undefined : cw(lang, "Sign-in failed");
  return undefined;
}
