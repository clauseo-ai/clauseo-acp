// Settings, Plan and usage (U12): each agent's plan from the window's limits store, Claude's then ChatGPT's, as the
// room's own card draws it (room/limits/plan-card-view.tsx): the plan, every window with its share left, its bar and
// its reset, and "Updated at {time}"; muted when stale, the name alone while loading, Try again when the plan sent
// nothing, no bar and no figure billed by use, no share for a window whose reset passed. An agent whose state waits on
// its words (missing_scope, live_only) has no entry, and an agent of no kind is never in the view. Opening the section
// asks the host for a read (`refresh_limits`, `shown`), which reads only figures older than 5 minutes. No timers:
// times are said against the last view's arrival, and the host sends a new view at each shown reset. Not here until
// the host gives them: Manage plan and Manage billing (H11), the month's figure, the retry lines, what the plan does
// at its limit (H16).
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { useEffect, useRef } from "react";
import { LivePlanCard } from "../../room/limits/plan-card-view.tsx";
import { PLAN_NAME, lookOf } from "../../room/limits/plan-model.ts";
import { useLimitsStore, useLimitsView } from "../../store/limits.ts";
import type { AgentKind } from "../../transport/protocol.ts";
import { Group } from "./parts.tsx";

/** The agents with a plan, in the design's order. */
const KINDS: readonly AgentKind[] = ["claude", "codex"];

export function Usage() {
  const w = useSettingsWords();
  const store = useLimitsStore();
  const view = useLimitsView();
  // once per opening, for every agent; the ref holds through StrictMode's second run of the effect
  const asked = useRef(false);
  useEffect(() => {
    if (asked.current || store === null) return;
    asked.current = true;
    store.refresh(undefined, "shown");
  }, [store]);
  const updated = (time: string) => w.s("Updated at {time}", { time });
  return (
    <div className="set-usage" data-testid="usage-section">
      {KINDS.map((kind) => {
        const entry = view[kind];
        if (entry === undefined || lookOf(entry) === "hidden") return null;
        return (
          <Group key={kind} title={PLAN_NAME[kind]}>
            <LivePlanCard kind={kind} updated={updated} testId={`usage-${kind}`} />
          </Group>
        );
      })}
    </div>
  );
}
