// Where Clauseo Case Law's credit stands, one rule for every place that speaks of it (24 Nov round, r7363): Settings ›
// Connectors, the library's Case Law card, and the lines by the composer. "out" while the host's status is
// out_of_credit, whatever figure was last read (the Windows tester saw "Out of case-law credits" by the composer and
// "7 credits left" in Settings with nothing saying "out"), and also signed in at 0 credits, a never-funded account
// included, which never reads "Ready to search." or "Running low". "low" signed in under the host's low mark with
// credits left; "ok" otherwise signed in; null while it isn't signed in, connectors are off or it isn't listed. A
// figure only when the current status carries one: out_of_credit carries none, so none is shown with it. (A read above 0 clears the host's out-of-credit state, crates/host/src/
// connectors/state.rs set_account, even under one search's price: then every place says "Running low" together.)
import type { ConnectorView, ConnectorsState } from "../transport/protocol.ts";

/** The one connector at launch, Indian Kanoon through Clauseo's case-law service. */
export const CASE_LAW = "case-law";

export type CaseLawCredit =
  /** Out of credit, or at 0. `credits`: 0 when the current status reads it; null when it carries no figure. `payUrl`: the host's
   * checked pay link for the recharge it made (the out-of-credit status's, or the view's), null without one. */
  | { kind: "out"; credits: number | null; payUrl: string | null }
  /** Under the host's low mark with credits left; `credits` null before the balance is read. */
  | { kind: "low"; credits: number | null }
  | { kind: "ok" };

/** The connector as the rule reads it, while connectors are on. */
export const caseLawOf = (state: ConnectorsState): ConnectorView | undefined =>
  state.available ? state.connectors.find((c) => c.id === CASE_LAW) : undefined;

/** The credit, from the current state alone. */
export function caseLawCredit(state: ConnectorsState): CaseLawCredit | null {
  const c = caseLawOf(state);
  if (c === undefined || typeof c.status !== "object") return null;
  if ("out_of_credit" in c.status && c.status.out_of_credit !== undefined)
    return {
      kind: "out",
      credits: null,
      payUrl: c.status.out_of_credit.pay_url ?? c.pay_url ?? null,
    };
  if (!("signed_in" in c.status) || c.status.signed_in === undefined) return null;
  const signed = c.status.signed_in;
  const credits = signed.balance?.credits ?? null;
  if (credits !== null && credits <= 0)
    return { kind: "out", credits: 0, payUrl: c.pay_url ?? null };
  return signed.low ? { kind: "low", credits } : { kind: "ok" };
}

/** Whether two credits say the same, for the store to keep one object while nothing changed. */
export const sameCredit = (a: CaseLawCredit | null, b: CaseLawCredit | null): boolean =>
  JSON.stringify(a) === JSON.stringify(b);
