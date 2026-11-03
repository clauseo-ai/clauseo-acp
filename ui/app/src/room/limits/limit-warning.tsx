// The composer's warning (U12): a quiet note under the field, never blocking, when a session window reaches 95% used or
// the agent itself warns (tone `near` under 75%). "Close to your 5-hour limit. Long tasks may pause until 6:10 pm.",
// the reset the host's `resets_at` in a <time>. A part of its own, which reads the store itself, so a view renders the
// note alone and only when the warning's window or reset changes. The composer's container passes it as the notice;
// where the thread is paused or stopped at the limit, that line wins: `notice={stopped ?? <LimitWarning />}`.
// FieldNotice comes in statically: its module, lazy, would share a chunk with the thread's (stopped-mark.tsx imports
// it), which the release build's bare dynamic imports can't load (vite-config.test.ts).
import { FieldNotice } from "@clauseo/components/clauseo/limit-note.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { useAgentSelect } from "../../store/limits.ts";
import type { AgentLimitsView } from "../../transport/protocol.ts";
import { useAgentPlan } from "../thread/agent-plan.ts";
import { isoOf } from "../thread/clock.ts";
import { warningKey, warningWords } from "./plan-model.ts";

/** The warning's key with the moment its view arrived, the "now" its time is said against; "" for none. */
const keyAt = (entry: AgentLimitsView | undefined, at: number): string => {
  const key = warningKey(entry);
  return key === "" ? "" : `${at} ${key}`;
};

/** The open conversation's agent's warning; nothing when there is none. */
export function LimitWarning() {
  const plan = useAgentPlan();
  const keyed = useAgentSelect(plan?.kind, keyAt);
  const { lang } = useDisplay();
  if (keyed === "") return null;
  // its time against the view's arrival: the host sends a new view at each shown reset, nothing ticks here
  const space = keyed.indexOf(" ");
  const words = warningWords(lang, keyed.slice(space + 1), Number(keyed.slice(0, space)));
  if (words === null) return null;
  const until = words.until;
  return (
    <div className="contents" data-testid="limit-warning">
      <FieldNotice
        lead={words.lead}
        action={
          until === null ? null : (
            <time className="lim-field-lead" dateTime={isoOf(until.at)}>
              <Say>{until.text}</Say>
            </time>
          )
        }
      />
    </div>
  );
}
