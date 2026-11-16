// The plan's card, opened from the plan in the toolbar: a compact sibling of Settings' Plan and usage. Every window the
// plan has, the 5-hour and the weekly, each a raised tile with its label, the share left as a numeral, its bar filled
// with what's left as the toolbar's meter is, and when it resets; how fresh the figures are, what happens at the limit,
// whether preparing ahead is paused, Manage plan, and the way to Plan and usage in Settings. All of it in concrete:
// the plan's figures are information, not a warning, and a limit is never red. Parts hold no strings: the words come
// in already in the language.
import { Meter as BaseMeter } from "@base-ui/react/meter";
import type { ReactNode } from "react";
import { IconOpen, IconPlanLimit } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { Button } from "../ui/button.tsx";
import { Say } from "./say.tsx";

/** One window of the plan: "5-hour limit", "18% left", "Resets 6:10 pm". left is the share left, 0 to 100, the bar
 * filled with it as the toolbar's meter is; null when there are no figures, and no bar is drawn, so it never reads as
 * nothing left. */
export type PlanWindow = {
  name: string;
  left: number | null;
  leftText: string;
  reset: string;
  valueText: string;
  /** Optional: close to the limit, by the toolbar's band. The words and the fill take primary, never amber, never red. */
  close?: boolean;
};

export type PlanCardProps = {
  /** The plan's name, as the account gives it. */
  plan: string;
  /** "As of 4:28 pm": a clock time, never a relative one. */
  updated: string | null;
  /** The figures are old or the last read failed: they go muted, and `message` says so. */
  stale?: boolean;
  windows: readonly PlanWindow[];
  /** A line in place of, or over, the windows: "Couldn't refresh just now. Clauseo will try again." */
  message?: { title: string; body?: string | null; action?: ReactNode } | null;
  /** Preparing ahead, and where it stands. */
  preparing?: { title: string; text: string; link?: ReactNode } | null;
  /** "At the limit, Claude stops where it is...". Null where there's no limit to reach, billed by use. */
  foot: string | null;
  /** Optional: Manage plan or Manage billing, the vendor's own page. Left out until the host can open it (H11). */
  manage?: { label: string; href: string } | null;
  /** Optional: Plan and usage in Settings, opened at its section. Left out where the host can't open Settings. */
  settings?: { label: string; onOpen: () => void } | null;
  /** Optional: a test id on the card's body. */
  testId?: string;
};

/** "98% left" as the numeral and the rest beside it, as Settings' tiles say it; the whole line as words where the
 * language doesn't lead with the numeral. */
function splitLeft(left: number, text: string): { numeral: string; rest: string } {
  const numeral = `${left}%`;
  return text.startsWith(numeral)
    ? { numeral, rest: text.slice(numeral.length).trim() }
    : { numeral: "", rest: text };
}

function WindowRow({ w, stale }: { w: PlanWindow; stale: boolean }) {
  const left = w.left === null ? null : splitLeft(w.left, w.leftText);
  return (
    <div
      className="plan-window"
      data-state={w.close === true ? "close" : "plenty"}
      data-stale={stale ? "" : undefined}
    >
      <BaseMeter.Root
        value={w.left ?? 0}
        aria-valuetext={w.valueText}
        className="plan-window-meter"
      >
        <BaseMeter.Label className="plan-window-name">
          <Say>{w.name}</Say>
        </BaseMeter.Label>
        {left !== null && (
          <span className="plan-window-used">
            {left.numeral !== "" && (
              <span className="plan-numeral">
                <Say>{left.numeral}</Say>
              </span>
            )}{" "}
            <span className="plan-window-word">
              <Say>{left.rest}</Say>
            </span>
          </span>
        )}
        {w.left !== null && (
          <BaseMeter.Track className="plan-track">
            <BaseMeter.Indicator className="plan-fill" />
          </BaseMeter.Track>
        )}
      </BaseMeter.Root>
      {w.reset !== "" && (
        <p className="plan-window-reset">
          <Say>{w.reset}</Say>
        </p>
      )}
    </div>
  );
}

/** The card's contents: it stands inside the toolbar's popover, or at rest in settings. */
export function PlanCard({
  plan,
  updated,
  stale = false,
  windows,
  message,
  preparing,
  foot,
  manage,
  settings,
  testId,
}: PlanCardProps) {
  return (
    <div className="plan-card-body" data-testid={testId}>
      {/* a div, not a header: in the popup's portal a header would read as a second banner beside the toolbar's */}
      <div className="plan-card-head">
        <p className="plan-card-name">
          <Say>{plan}</Say>
        </p>
        {updated !== null && (
          <p className={cn("plan-card-updated", stale && "plan-card-stale")}>
            <Say>{updated}</Say>
          </p>
        )}
      </div>
      {message != null && (
        <div className="plan-card-message" role="status">
          <p className="plan-card-message-title">
            <Say>{message.title}</Say>
          </p>
          {message.body != null && message.body !== "" && (
            <p className="plan-card-message-body">
              <Say>{message.body}</Say>
            </p>
          )}
          {message.action}
        </div>
      )}
      {windows.length > 0 && (
        <div className="plan-windows">
          {windows.map((w) => (
            <WindowRow key={w.name} w={w} stale={stale} />
          ))}
        </div>
      )}
      {preparing != null && (
        <div className="plan-card-prep">
          <p className="plan-card-prep-title">
            <Say>{preparing.title}</Say>
          </p>
          <p className="plan-card-prep-text">
            <Say>{preparing.text}</Say> {preparing.link}
          </p>
        </div>
      )}
      {(foot !== null || manage != null) && (
        <div className="plan-card-foot">
          {foot !== null && (
            <p>
              <Say>{foot}</Say>
            </p>
          )}
          {manage != null && (
            <a className="plan-card-manage" href={manage.href} target="_blank" rel="noreferrer">
              <span className="deva-nudge">
                <Say>{manage.label}</Say>
              </span>
              <IconOpen size={14} />
            </a>
          )}
        </div>
      )}
      {settings != null && (
        <div className="plan-card-go">
          <Button kind="tone" size="small" data-testid="plan-settings" onClick={settings.onOpen}>
            <IconPlanLimit size={16} />
            {settings.label}
          </Button>
        </div>
      )}
    </div>
  );
}
