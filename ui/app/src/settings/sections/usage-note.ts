// Plan and usage's note beside its name in the side list (U12), from the window's limits view, pure: the design's
// useNotes (design/03-system/src/pages/settings/settings-window.tsx) with the host's figures.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { lookOf } from "../../room/limits/plan-model.ts";
import { windowOf } from "../../room/limits/plan-windows.ts";
import type { AgentKind, LimitsView } from "../../transport/protocol.ts";

/** The agents with a plan, in the design's order. */
export const KINDS: readonly AgentKind[] = ["claude", "codex"];

/** What Plan and usage says beside its name: the share left of Claude's 5-hour window (else the first agent's with
 * figures), "Pay as you go" for an agent billed by use, nothing while no agent has figures. */
export function usageNote(view: LimitsView, lang: Lang): string | undefined {
  for (const kind of KINDS) {
    const entry = view[kind];
    if (entry === undefined) continue;
    const look = lookOf(entry);
    if (look === "payg") return st(lang, "Pay as you go");
    if (look !== "figures" && look !== "stale") continue;
    const session = entry.windows.find((w) => w.kind === "session");
    const left = session === undefined ? null : windowOf(lang, session, 0).left;
    if (left !== null) return st(lang, "{n}% of the 5-hour limit left", { n: left });
  }
  return undefined;
}
