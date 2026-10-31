import { useRef } from "react";
import type { ReactNode } from "react";
import { Popover } from "@base-ui/react/popover";
import { floating } from "../ui/recipes.ts";
import { Button } from "../ui/button.tsx";
import { SearchField } from "../ui/field.tsx";
import { PlanMeter, type PlanState } from "../ui/meter.tsx";
import { Toolbar, ToolbarPair, ToolbarPairButton } from "../ui/toolbar.tsx";
import { IconHistory, IconMatters, IconUndo } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useDisplay, type Platform } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { rt } from "../lib/words-room.ts";
import { useVersionWords } from "../lib/words-versions.ts";
import { HistoryPanel } from "./history.tsx";
import type { HistoryRow } from "./matter.ts";
import { CaptionButtons, CornerLights } from "./caption.tsx";
import { useRoom } from "./room-scope.ts";
import { mt } from "../lib/words-matters.ts";
import { Dot, Say } from "./say.tsx";
import { Heading } from "./heading.tsx";
import { NotProtectedMark } from "./allowed.tsx";

export type RoomToolbarProps = {
  /** The matter's name as filed, in English in both languages. */
  title: string;
  /** "{court}. {caseNo}. {forWhom}", the title's hint. */
  titleHint: string;
  nextDate: string;
  caseNo: string;
  court: string;
  /** The plan: its name, the share left, that share in words, and the state's line, "Plenty left until the 9:40 pm
   * reset". state follows the share unless given: close to the limit from 75% used, that is under a quarter left,
   * reached at nothing. Null before the lawyer has signed in to a plan, as on first launch: the plan stands only once
   * there is one. */
  plan: {
    name: string;
    left: number;
    valueText: string;
    status: string;
    state?: PlanState;
    /** Optional: how the plan's figures stand. stale: the last read is old or failed, so the figures go
     * muted; none: no figures to show, the meter an empty track; bare: an account billed by use, with no meter. */
    look?: "stale" | "none" | "bare";
    /** Optional: the plan's card, every window with its reset, opened by pressing the plan. */
    card?: ReactNode;
    /** The plan pressed: what pressing it does, "Show the plan's limits". Its accessible name starts with the words it
     * shows, the plan's name and its line, and ends with these. */
    cardLabel?: string;
    /** Whether the card stands open as the page loads. */
    cardOpen?: boolean;
  } | null;
  history: readonly HistoryRow[];
  /** Whether there's an act of the lawyer's to take back: accepting the answer. */
  canUndo: boolean;
  /** Clauseo is writing: Undo waits, faint, until it's done. */
  writing?: boolean;
  onUndo: () => void;
  railOpen: boolean;
  onRail: () => void;
  /** The window's chrome, where it isn't the computer's own. Defaults to the computer's platform. */
  platform?: Platform;
  /** Words in the next date's place, for a matter with no date, such as the scratch matter's "Quick questions". */
  subtitle?: string;
  /** Optional: the current matter isn't protected. Its line carries the short Not protected mark, last. */
  unprotected?: boolean;
  /** Optional: a matter on the rail waits on the lawyer, with a question or for permission. While the rail is
   * folded behind Matters, the button carries one 8 px court-yellow dot, so the wait is seen without opening it. */
  railWaits?: boolean;
  /** Optional: the search field's name where the room isn't a matter's, such as "Search quick questions". */
  searchLabel?: string;
};

/** The plan's state from the share left, where the host hasn't said. */
const planState = (left: number): PlanState =>
  left <= 0 ? "reached" : left <= 0.25 ? "close" : "plenty";

/** The toolbar: the matter as the header, then search, Versions and Undo, and the plan. 64 px on the ground, and the
 * window's title bar on both platforms: its empty space drags the window (data-tauri-drag-region, which Tauri reads
 * on the element pressed, not its children). The corner lights lead it on the Mac; on Windows the plan stands left
 * of the caption buttons, which end it at the top right. */
export function RoomToolbar(p: RoomToolbarProps) {
  const w = useWords();
  const vw = useVersionWords();
  const display = useDisplay();
  const platform = p.platform ?? display.platform;
  // Windows and Linux: the caption buttons at the top right, and no corner lights
  const captions = platform !== "mac";
  const room = useRoom();
  const undo = useRef<HTMLButtonElement>(null);
  // Versions opens under the pair, its right edge on the pair's: Undo's own group
  const pairOf = () => undo.current?.parentElement ?? null;
  const writing = p.writing === true;
  return (
    <header
      className={cn("room-top", captions && "mid:pr-0")}
      data-platform={platform}
      data-tauri-drag-region
    >
      <div className="room-lead" data-tauri-drag-region>
        {platform === "mac" && <CornerLights />}
        <Button
          kind="tone"
          size="small"
          className={cn("room-rail-toggle", p.railWaits === true && "room-rail-waits")}
          aria-expanded={p.railOpen}
          aria-controls={room.id("rail")}
          onClick={p.onRail}
        >
          <IconMatters size={16} />
          {w.t("Matters")}
          {p.railWaits === true && (
            <>
              <i className="room-rail-wait" aria-hidden="true" />
              <span className="sr-only">{mt(w.lang, "A matter waits for you")}</span>
            </>
          )}
        </Button>
      </div>
      <div className="room-matter" data-tauri-drag-region>
        <Heading
          level={1}
          className="room-title as-en"
          lang="en"
          title={p.titleHint}
          data-tauri-drag-region
        >
          {p.title}
        </Heading>
        {p.subtitle !== undefined ? (
          <p className="room-next" data-tauri-drag-region>
            <Say>{p.subtitle}</Say>
            {p.unprotected === true && (
              <span className="room-case">
                <Dot />
                <NotProtectedMark short />
              </span>
            )}
          </p>
        ) : (
          <p className="room-next" data-tauri-drag-region>
            <span>{w.t("Next date")}</span> <span className="room-next-value">{p.nextDate}</span>
            {p.caseNo !== "" && (
              <span className="room-case" title={p.court}>
                <Dot />
                <span>
                  <Say>{p.caseNo}</Say>
                </span>
              </span>
            )}
            {p.unprotected === true && (
              <span className="room-case">
                <Dot />
                <NotProtectedMark short />
              </span>
            )}
          </p>
        )}
      </div>
      {/* on Windows the caption buttons take the toolbar's last 158 px, so the tools give way a hundred times faster
          than the title, which then never loses the fraction of a pixel that would wrap it */}
      <div className={cn("room-tools", captions && "shrink-100")} data-tauri-drag-region>
        {/* flex-nowrap: the room's toolbar folds its search instead of wrapping its tools onto a second line */}
        <Toolbar label={rt(w.lang, "Tools for this matter")} className="room-tools-bar flex-nowrap">
          <SearchField
            label={p.searchLabel ?? w.t("Search this matter")}
            place="toolbar"
            className={cn("room-search", captions && "max-mid:ml-auto max-narrow:ml-0")}
          />
          <Popover.Root
            onOpenChange={(open, details) => {
              // Undo keeps Versions up, so the row it takes out can be seen to go
              const t = details.event?.target;
              if (
                !open &&
                details.reason === "outside-press" &&
                t instanceof Node &&
                undo.current?.contains(t) === true
              ) {
                details.cancel();
              }
            }}
          >
            <ToolbarPair label={w.t("Versions and Undo")} className="room-pair">
              <ToolbarPairButton render={<Popover.Trigger />}>
                <IconHistory size={16} className="room-pair-icon" />
                <span className="deva-nudge">{vw.v("Versions")}</span>
              </ToolbarPairButton>
              {/* while Clauseo writes, Undo waits disabled and stays focusable, as Base UI's toolbar draws a disabled
                    button: aria-disabled and data-disabled. Set here rather than through `disabled`, which re-registers
                    the toolbar's items and costs a second commit in the frame the answer starts and the one it ends */}
              <ToolbarPairButton
                ref={undo}
                className="room-undo"
                aria-disabled={writing ? true : undefined}
                data-disabled={writing ? "" : undefined}
                aria-label={p.canUndo ? w.t("Undo accepting the answer") : undefined}
                onClick={(e) => {
                  if (writing) e.preventDefault();
                  else p.onUndo();
                }}
              >
                <IconUndo size={16} className="room-pair-icon" />
                <span className="deva-nudge room-undo-label">{w.t("Undo")}</span>
              </ToolbarPairButton>
            </ToolbarPair>
            <HistoryPanel rows={p.history} anchor={pairOf} />
          </Popover.Root>
        </Toolbar>
        {p.plan !== null && <ToolbarPlan plan={p.plan} />}
      </div>
      {captions && (
        // flush with the window's top right: beside the plan on a wide window, on the first row of tools on a narrow
        // one, where the toolbar's own padding is taken back
        <CaptionButtons className="self-start mid:ml-s5 max-mid:order-4 max-mid:-mt-s3 max-mid:-mr-s4" />
      )}
    </header>
  );
}

/** The plan in the toolbar: the meter and its line, and, where the plan has a card, a press that opens it under the
 * plan, its right edge on the plan's. Pressed or not, the plan holds the width of its widest state. */
function ToolbarPlan({ plan }: { plan: NonNullable<RoomToolbarProps["plan"]> }) {
  const w = useWords();
  const nameText = plan.look === "bare" ? plan.name : w.t("{name} plan", { name: plan.name });
  const meter = (
    <PlanMeter
      name={<Say>{nameText}</Say>}
      value={plan.look === "none" || plan.look === "bare" ? 0 : Math.round(plan.left * 100)}
      valueText={plan.valueText}
      status={plan.status}
      state={plan.state ?? planState(plan.left)}
      track={plan.look !== "none" && plan.look !== "bare"}
      inButton={plan.card !== undefined}
      dragRegion
      className={cn("room-plan", plan.card !== undefined && "room-plan-in")}
    />
  );
  if (plan.card === undefined) {
    return plan.look === undefined ? (
      meter
    ) : (
      <div className="room-plan room-plan-look" data-look={plan.look}>
        {meter}
      </div>
    );
  }
  return (
    <Popover.Root {...(plan.cardOpen === true ? { defaultOpen: true } : {})}>
      {/* named by its own words, the plan's name and its line, as they show, so speech input can say what it sees; what
          pressing does follows them, for assistive technology only */}
      <Popover.Trigger className="room-plan room-plan-press" data-look={plan.look}>
        {meter}
        <span className="sr-only">{`${/[.\u0964]$/.test(plan.status) ? "" : "."} ${plan.valueText}${plan.cardLabel === undefined ? "" : `. ${plan.cardLabel}`}`}</span>
      </Popover.Trigger>
      {/* over the work and the wall, as Versions stands: its right edge on the plan's, 8 px below */}
      <Popover.Portal>
        <Popover.Positioner
          side="bottom"
          align="end"
          sideOffset={8}
          collisionPadding={8}
          positionMethod="fixed"
          className="z-30"
        >
          {/* opened for the review as the page loads, the card leaves the focus where it was */}
          <Popover.Popup
            className={cn(floating({ width: "history" }), "shadow-e3 plan-card")}
            aria-label={nameText}
            {...(plan.cardOpen === true ? { initialFocus: false } : {})}
          >
            {plan.card}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
