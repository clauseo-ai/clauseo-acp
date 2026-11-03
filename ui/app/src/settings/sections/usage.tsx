// Settings, Plan and usage (U12): each agent's plan from the window's limits store, Claude's then ChatGPT's, in the
// design's form (design/03-system/src/pages/settings/sections-a.tsx, AgentUsage): the plan as the group's head with
// "Updated at {time}" at its end, then each window as a raised tile, the share left as the wall's numeral, a meter
// filled with what's left, and the reset. The figures come from the room's own logic (room/limits/plan-model.ts,
// plan-windows.ts): a window whose reset passed shows no share and no meter, only when it reset, muted; from the
// toolbar's close band on, a tile's words take primary, never amber, never red. Stale: the figures muted. Loading: the
// plan's name alone. No figures: the card's message and Try again (`refresh_limits`, `try_again`). Billed by use: who
// bills, no figure. An agent whose state waits on its words (missing_scope, live_only) has no entry, and an agent of no
// kind is never in the view. Opening the section asks the host for a read (`shown`), which reads only figures older
// than 5 minutes. No timers: times are said against the clock at each render, and the host sends a new view at each
// shown reset. Not here until the host gives them: Manage plan and Manage billing (H11), the month's figure, the retry
// lines and what the plan does at its limit (H16), and preparing ahead (later).
import { IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { pc } from "@clauseo/components/lib/words-plan-card.ts";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useEffect, useRef } from "react";
import { PLAN_NAME, lookOf, planTitle } from "../../room/limits/plan-model.ts";
import { windowOf, type WindowFigures } from "../../room/limits/plan-windows.ts";
import { stamp } from "../../room/thread/clock.ts";
import { useLimitsStore, useLimitsView, useNow } from "../../store/limits.ts";
import type { AgentKind, AgentLimitsView } from "../../transport/protocol.ts";
import { Group, PlateRow } from "./parts.tsx";
import { KINDS } from "./usage-note.ts";

/** One window as the design's tile: its label, the share left as the numeral with the rest of "{n}% left" beside it,
 * the meter, and the reset; no numeral and no meter without a share. */
function WindowTile({ figures, stale }: { figures: WindowFigures; stale: boolean }) {
  const w = useSettingsWords();
  const { left } = figures;
  const numeral = left === null ? "" : `${left}%`;
  return (
    <div
      className="set-window-tile"
      data-state={figures.close ? "close" : "plenty"}
      data-stale={stale ? "" : undefined}
      data-passed={figures.passed ? "" : undefined}
    >
      <p className="set-window-label">{w.say(figures.name)}</p>
      {left !== null && (
        <>
          <p className="set-window-used">
            <span className="set-numeral">{numeral}</span>
            <span className="set-window-word">
              {w.say(figures.leftText.replace(numeral, "").trim())}
            </span>
          </p>
          {/* the design's meter, filled with what's left as PlanMeter's is; its width set as Base UI sets the
              room's, through the element's style */}
          <div
            className="set-meter"
            role="meter"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={left}
            aria-label={figures.name}
            aria-valuetext={figures.leftText}
          >
            <i className="set-meter-fill" style={{ width: `${left}%` }} />
          </div>
        </>
      )}
      {figures.reset !== "" && <p className="set-window-reset">{w.say(figures.reset)}</p>}
    </div>
  );
}

function AgentUsage({ kind, entry }: { kind: AgentKind; entry: AgentLimitsView }) {
  const w = useSettingsWords();
  const words = useWords();
  const store = useLimitsStore();
  // times against this render's clock: the host sends a new view at each shown reset, nothing ticks here
  const now = useNow();
  const { lang } = w;
  const look = lookOf(entry);
  const name = PLAN_NAME[kind];
  const plan = entry.plan ?? words.t("{name} plan", { name });
  const testId = `usage-${kind}`;
  if (look === "payg")
    return (
      <Group title={name}>
        <PlateRow>
          <div data-testid={testId}>
            <p className="text-ui text-primary">
              {w.say(
                entry.provider === "anthropic"
                  ? pc(lang, "Pay as you go, billed to your Anthropic account")
                  : planTitle(lang, kind, look),
              )}
            </p>
            <p className="set-note">
              {w.say(w.s("No limits to show. Each question is billed to the key's account."))}
            </p>
          </div>
        </PlateRow>
      </Group>
    );
  if (look === "none")
    return (
      <Group title={plan}>
        <PlateRow className="set-choice">
          <div className="set-choice-words" data-testid={testId} role="status">
            <p className="text-ui text-primary">
              {w.say(pc(lang, "Usage isn't available right now."))}
            </p>
            <p className="set-note">
              {w.say(
                pc(
                  lang,
                  "Your {name} plan didn't send its limits this time. Asking still works as usual.",
                  { name },
                ),
              )}
            </p>
          </div>
          <Button
            kind="tone"
            size="small"
            className="flex-none"
            data-testid="plan-try-again"
            onClick={() => store?.refresh(kind, "try_again")}
          >
            <IconTryAgain size={16} />
            {lim(lang, "Try again")}
          </Button>
        </PlateRow>
      </Group>
    );
  // loading: the plan's name alone, no figures
  if (look === "loading")
    return (
      <Group title={plan} plate={false}>
        <div data-testid={testId} data-look="loading" />
      </Group>
    );
  const stale = look === "stale";
  return (
    <Group
      title={plan}
      aside={
        entry.checked_at === undefined ? undefined : (
          <span className="set-aside-time" data-stale={stale ? "" : undefined}>
            {w.say(w.s("Updated at {time}", { time: stamp(lang, entry.checked_at, now) }))}
          </span>
        )
      }
      plate={false}
    >
      <div className="set-windows" data-testid={testId}>
        {entry.windows.map((row) => (
          <WindowTile key={row.id} figures={windowOf(lang, row, now)} stale={stale} />
        ))}
      </div>
    </Group>
  );
}

export function Usage() {
  const store = useLimitsStore();
  const view = useLimitsView();
  // once per opening, for every agent; the ref holds through StrictMode's second run of the effect
  const asked = useRef(false);
  useEffect(() => {
    if (asked.current || store === null) return;
    asked.current = true;
    store.refresh(undefined, "shown");
  }, [store]);
  return (
    <div className="set-usage" data-testid="usage-section">
      {KINDS.map((kind) => {
        const entry = view[kind];
        if (entry === undefined || lookOf(entry) === "hidden") return null;
        return <AgentUsage key={kind} kind={kind} entry={entry} />;
      })}
    </div>
  );
}
