// The plan's card, opened from the plan in the toolbar. Every window the plan has, the 5-hour and the weekly, each
// with how much is left and when it resets, its bar filled with what's left as the toolbar's meter is, how fresh the
// figures are, what happens at the limit, whether preparing ahead is paused, and Manage plan. All of it in concrete:
// the plan's figures are information, not a warning, and a limit is never red. Parts hold no strings: the words come
// in already in the language.
import { Meter as BaseMeter } from "@base-ui/react/meter";
import type { ReactNode } from "react";
import { IconOpen } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
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
  manage: { label: string; href: string };
};

function WindowRow({ w, stale }: { w: PlanWindow; stale: boolean }) {
  return (
    <div className="plan-window" data-stale={stale ? "" : undefined}>
      <BaseMeter.Root
        value={w.left ?? 0}
        aria-valuetext={w.valueText}
        className="plan-window-meter"
      >
        <span className="plan-window-line">
          <BaseMeter.Label className="plan-window-name">
            <Say>{w.name}</Say>
          </BaseMeter.Label>
          {w.left !== null && (
            <span className="plan-window-used">
              <Say>{w.leftText}</Say>
            </span>
          )}
        </span>
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
}: PlanCardProps) {
  return (
    <div className="plan-card-body">
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
      <div className="plan-card-foot">
        {foot !== null && (
          <p>
            <Say>{foot}</Say>
          </p>
        )}
        <a className="plan-card-manage" href={manage.href} target="_blank" rel="noreferrer">
          <span className="deva-nudge">
            <Say>{manage.label}</Say>
          </span>
          <IconOpen size={14} />
        </a>
      </div>
    </div>
  );
}
