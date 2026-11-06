// The Connectors section's logic apart from its parts (U15), so each part file exports components alone: the status's
// kind, the pay link's guard, a pack's line, the spend's days and the agents' names. In the section's chunk.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { count, rupees } from "@clauseo/components/lib/money.ts";
import { listOf } from "@clauseo/components/lib/words-core.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { PLAN_NAME } from "../../room/limits/plan-model.ts";
import type {
  AgentKind,
  ConnectorStatus,
  LimitsView,
  PackView,
  RecentSpend,
} from "../../transport/protocol.ts";

/** How long after "Continue to payment" a new pay link may open by itself. */
export const PAY_GUARD_MS = 30_000;

/** The status's kind, as the block switches on it. */
export type Kind =
  | "not_added"
  | "signing_in"
  | "signed_in"
  | "out_of_credit"
  | "signed_out"
  | "key_revoked"
  | "error";

export function kindOf(status: ConnectorStatus): Kind {
  if (typeof status === "string") return status;
  if ("signed_in" in status && status.signed_in !== undefined) return "signed_in";
  if ("out_of_credit" in status && status.out_of_credit !== undefined) return "out_of_credit";
  return "error";
}

/** A pack's one line: its credits, then its price. */
export const packLabel = (lang: Lang, pack: PackView): string =>
  cw(lang, "{credits} credits, {rupees}", {
    credits: count(pack.credits),
    rupees: rupees(pack.paise),
  });

const DAY = /^(\d{4})-(\d{2})-(\d{2})$/;

/** The local calendar date of a moment, as the account writes days. */
export function dateOf(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** A calendar date as its line names it, "Wednesday 4 November"; null when it isn't a real date. A date is no
 * moment, so it reads in no time zone. */
export function dayName(lang: Lang, day: string): string | null {
  const m = DAY.exec(day);
  if (m === null) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const at = new Date(Date.UTC(y, mo - 1, d));
  if (at.getUTCFullYear() !== y || at.getUTCMonth() !== mo - 1 || at.getUTCDate() !== d)
    return null;
  return new Intl.DateTimeFormat(lang === "hi" ? "hi-IN-u-nu-latn" : "en-GB", {
    weekday: "long",
    day: "numeric",
    month: "long",
    timeZone: "UTC",
  }).format(at);
}

export type SpendRow = { day: string; label: string; credits: number };

/** The rows recent spend shows: today first, then the other readable days, newest first. */
export function spendRows(lang: Lang, spend: RecentSpend, today: string): SpendRow[] {
  const days = spend.days
    .filter((d) => d.day !== today)
    .map((d) => ({ day: d.day, label: dayName(lang, d.day), credits: d.credits }))
    .filter((d): d is SpendRow => d.label !== null)
    .sort((a, b) => (a.day < b.day ? 1 : a.day > b.day ? -1 : 0));
  return [{ day: today, label: cw(lang, "Today"), credits: spend.today_credits }, ...days];
}

const KINDS: readonly AgentKind[] = ["claude", "codex"];

/** The agents' names for "Clauseo asks {name} ...": those the window has limits for, else both, since either may run
 * a case-law search. */
export function agentNames(lang: Lang, limits: LimitsView): string {
  const known = KINDS.filter((k) => limits[k] !== undefined);
  return listOf(
    lang,
    (known.length > 0 ? known : KINDS).map((k) => PLAN_NAME[k]),
  );
}
