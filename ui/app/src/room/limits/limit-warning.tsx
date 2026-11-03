// The composer's warning (U12): a quiet note under the field, never blocking, when a session window reaches 95% used or
// the agent itself warns (tone `near` under 75%). "Close to your 5-hour limit. Long tasks may pause until 6:10 pm.",
// the reset the host's `resets_at` in a <time>. A part of its own, which reads the store itself, so a view renders the
// note alone and only when the warning's window or reset changes. The composer passes it as its notice, and leaves it
// out while the thread is paused at the limit, when the status line says so (work-composer.tsx).
// The note itself is in the thread's chunk (thread/limit-warning-note.tsx, re-exported by list.tsx), where
// limit-note.tsx already is: loaded lazily on its own it would share a chunk with the thread's, which the release
// build's bare dynamic imports can't load (vite-config.test.ts).
import { lazy, Suspense } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { useAgentSelect } from "../../store/limits.ts";
import type { AgentLimitsView } from "../../transport/protocol.ts";
import { useAgentPlan } from "../thread/agent-plan.ts";
import { warningKey } from "./plan-model.ts";

/** From the thread's chunk, which the open thread has loaded already. */
const WarningNote = lazy(() =>
  late(import("../thread/list.tsx")).then((m) => ({ default: m.WarningNote })),
);

/** The warning's key with the moment its view arrived, the "now" its time is said against; "" for none. */
const keyAt = (entry: AgentLimitsView | undefined, at: number): string => {
  const key = warningKey(entry);
  return key === "" ? "" : `${at} ${key}`;
};

/** The open conversation's agent's warning; nothing when there is none. */
export function LimitWarning() {
  const plan = useAgentPlan();
  const keyed = useAgentSelect(plan?.kind, keyAt);
  if (keyed === "") return null;
  // its time against the view's arrival: the host sends a new view at each shown reset, nothing ticks here
  const space = keyed.indexOf(" ");
  return (
    <Suspense fallback={null}>
      <WarningNote warning={keyed.slice(space + 1)} now={Number(keyed.slice(0, space))} />
    </Suspense>
  );
}
