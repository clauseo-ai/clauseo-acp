// Plan and usage's note beside its name in the side list (U12), from the window's limits view, pure: the design's
// useNotes (design/03-system/src/pages/settings/settings-window.tsx) with the host's figures.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { PLAN_NAME, lookOf } from "../../room/limits/plan-model.ts";
import { windowOf } from "../../room/limits/plan-windows.ts";
import type { AgentKind, LimitsView } from "../../transport/protocol.ts";
import type { NotSetUp } from "../plan-setup.ts";

/** The agents with a plan, in the design's order. */
export const KINDS: readonly AgentKind[] = ["claude", "codex"];

/** What Plan and usage says beside its name: each agent's plan named, in the design's order, so a ChatGPT matter
 * never reads Claude's figure as its own (Windows test, 24 Nov): "Claude: 17% left · ChatGPT: 60% left", the share left
 * of each 5-hour window; "Claude: pay as you go" for one billed by use and set up; an agent with no figures left out;
 * nothing while none has any. The settings window doesn't know which agent a matter uses, so it names both. */
export function usageNote(
  view: LimitsView,
  lang: Lang,
  notSetUp: NotSetUp = () => false,
): string | undefined {
  const parts: string[] = [];
  for (const kind of KINDS) {
    const entry = view[kind];
    if (entry === undefined) continue;
    const look = lookOf(entry);
    const name = PLAN_NAME[kind];
    // a plan with no sign-in reads as billed by use (no subscription, no limits): not set up, so nothing (D-060)
    if (look === "payg" && notSetUp(kind)) continue;
    if (look === "payg") {
      parts.push(st(lang, "{name}: pay as you go", { name }));
      continue;
    }
    if (look !== "figures" && look !== "stale") continue;
    const session = entry.windows.find((w) => w.kind === "session");
    const left = session === undefined ? null : windowOf(lang, session, 0).left;
    if (left !== null) parts.push(st(lang, "{name}: {n}% left", { name, n: left }));
  }
  return parts.length === 0 ? undefined : parts.join(" · ");
}
