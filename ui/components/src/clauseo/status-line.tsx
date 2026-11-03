// The status line on the composer's top edge: how long the answer took, the plan the questions go to, and the
// prepared draft's hint while its Ready card is out of view.
import {
  lazy,
  Suspense,
  useId,
  useLayoutEffect,
  useRef,
  type PointerEvent,
  type ReactNode,
} from "react";
import { late } from "../lib/late.ts";
import { Kbd } from "../ui/kbd.tsx";
import { SegmentedSwitch } from "../ui/segmented.tsx";
import { motionAllowed, useDisplay } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { ui } from "../lib/words-ui.ts";
import { useWide } from "./layout.ts";
import { nextStatusFit, type StatusFit } from "./logic.ts";
import { dur, ease } from "./motion.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";

export type ReadyHint = {
  /** "Draft petition ready", or "Draft petition, opened" once turned to. */
  what: string;
  label: string;
  state: "ready" | "opened";
  onTurn: () => void;
};

/** The plan the questions go to, chosen from the status line. */
export type PlanChoice<V extends string = string> = {
  value: V;
  options: readonly { value: V; label: string }[];
  onValueChange: (value: V) => void;
  /** Optional: drawn as the segmented switch, every plan in view, Claude | ChatGPT, after "Plan to ask with", where
   * choosing is the point, as before a new conversation's first message. Each option's short name. */
  segmented?: readonly { value: V; label: string }[];
};

/** The tool line, after the plan: what Clauseo's tools can do in the open conversation, one phrase, and the control
 * that opens where each tool stands. `summary` is the phrase it says, for the fit: the line measures again when it
 * changes. */
export type ToolsSegment = { summary: string; node: ReactNode };

/** The plan's select, loaded only for a status line that offers a choice. Until it is in, the plan stands as the
 * status line's own words, as the select's trigger does at rest. */
const PlanTrigger = lazy(() =>
  late(import("./plan-trigger.tsx").then((m) => ({ default: m.PlanTrigger }))),
) as unknown as typeof import("./plan-trigger.tsx").PlanTrigger;

/** The status line on the composer's top edge: "Took 38 seconds", a dot, "Uses your Claude plan", and the ready hint
 * at its right while the Ready card is out of view. Before the first answer it holds the plan alone. It never
 * truncates: the hint gives up its words first and keeps its key cap; if that still isn't enough the line wraps to
 * two. When the working line closes into it, the time comes up into place. */
export function StatusLine<V extends string>({
  took,
  plan,
  hint,
  choice,
  tools = null,
  hidden = false,
}: {
  took: string | null;
  plan: string;
  hint: ReadyHint | null;
  choice?: PlanChoice<V> | undefined;
  /** The tool line, parted from the plan by the dot; null for none. */
  tools?: ToolsSegment | null;
  /** While the working line stands in its place. It stays mounted, so the plan's select keeps its state. */
  hidden?: boolean;
}) {
  const w = useWords();
  const { lang, textSize } = useDisplay();
  const wide = useWide();
  const box = useRef<HTMLDivElement>(null);
  const line = useRef<HTMLDivElement>(null);
  const tookRef = useRef<HTMLSpanElement>(null);
  const goRef = useRef<HTMLSpanElement>(null);
  const toolsRef = useRef<HTMLSpanElement>(null);
  const hadTook = useRef(took !== null);
  const seg = useId();
  const { id, hand } = useRoom();
  const hintState = hint?.state ?? "none";
  const toolsSaid = tools?.summary ?? null;

  // the time it took arrives as the working line closes: it comes up 4 px into place over 200 ms
  useLayoutEffect(() => {
    const was = hadTook.current;
    hadTook.current = took !== null;
    if (was || took === null || !motionAllowed()) return;
    tookRef.current?.animate(
      [
        { opacity: 0, transform: "translateY(4px)" },
        { opacity: 1, transform: "none" },
      ],
      {
        duration: dur("state"),
        easing: ease("arrive"),
      },
    );
  }, [took]);

  // measured from the full line each time the words, the hint or the width change: the words' own line gives way
  // to the hint, so its words standing past its box is what says the line is full
  useLayoutEffect(() => {
    const b = box.current;
    const l = line.current;
    if (b === null || l === null || hidden) return;
    const fit = () => {
      let f: StatusFit = "full";
      b.dataset["fit"] = f;
      while (f !== "wrapped" && l.scrollWidth > l.clientWidth) {
        // the hint's key cap closes its "Turn to it"
        const cap = goRef.current?.lastElementChild ?? null;
        f = nextStatusFit(f, cap !== null && getComputedStyle(cap).display !== "none");
        b.dataset["fit"] = f;
      }
      // a line that wraps never starts a row with the tool line's dot: the dot parts two phrases on one row
      const t = toolsRef.current;
      if (t !== null) {
        delete t.dataset["lead"];
        const box = l.getBoundingClientRect();
        const lead = box.width > 0 && t.getBoundingClientRect().left - box.left < 1;
        if (lead && t.previousElementSibling !== null) t.dataset["lead"] = "";
      }
    };
    fit();
    const host = b.parentElement ?? b;
    let width = host.clientWidth;
    const ro = new ResizeObserver(() => {
      if (host.clientWidth === width) return;
      width = host.clientWidth;
      fit();
    });
    ro.observe(host);
    return () => ro.disconnect();
  }, [lang, textSize, wide, hintState, took, hidden, toolsSaid]);

  const hold = (on: boolean) => (e?: PointerEvent) => {
    if (e?.pointerType !== "touch") hand.holdDraft("hint", on);
  };
  return (
    <div className="room-status" ref={box} hidden={hidden}>
      {/* a div, since the plan's switch is no phrasing content; only the words are a live region, never the switch,
          their box drawn as none so the line lays them out as its own */}
      <div className="room-status-line" ref={line}>
        <span className="contents" role="status">
          {took !== null && (
            <span className="room-status-took" ref={tookRef}>
              <b>{took}</b>
              <i className="room-dot" aria-hidden="true" />
            </span>
          )}
          {choice === undefined && (
            <span className="room-status-plan">
              <Say>{plan}</Say>
            </span>
          )}
        </span>
        {choice === undefined ? null : choice.segmented !== undefined ? (
          <span className="room-status-plan room-status-seg">
            {/* the visible words name the switch, so they are read once, as its name */}
            <span className="deva-nudge" id={seg}>
              {ui(lang, "Plan to ask with")}
            </span>
            <SegmentedSwitch<V>
              labelledBy={seg}
              value={choice.value}
              options={choice.segmented}
              onValueChange={choice.onValueChange}
              className="room-plan-seg"
            />
          </span>
        ) : (
          <Suspense
            fallback={
              <span className="room-status-plan room-plan">
                <Say>{plan}</Say>
              </span>
            }
          >
            <PlanTrigger plan={plan} choice={choice} />
          </Suspense>
        )}
        {tools !== null && (
          <span className="room-status-tools" ref={toolsRef}>
            {(choice !== undefined || plan !== "" || took !== null) && (
              <i className="room-dot" aria-hidden="true" />
            )}
            {tools.node}
          </span>
        )}
      </div>
      {hint !== null && (
        <button
          type="button"
          className="room-hint"
          data-state={hint.state}
          aria-keyshortcuts="ArrowRight"
          aria-describedby={id("ready-hint-more")}
          onClick={hint.onTurn}
          onPointerEnter={hold(true)}
          onPointerLeave={hold(false)}
          onFocus={(e) => {
            if (e.currentTarget.matches(":focus-visible")) hand.holdDraft("hint", true);
          }}
          onBlur={() => hand.holdDraft("hint", false)}
        >
          <i className="room-hint-dot" aria-hidden="true" />
          <span className="room-hint-what deva-nudge">{hint.what}</span>
          <span className="room-hint-go" ref={goRef}>
            <span className="room-hint-turn deva-nudge">{w.t("Turn to it")}</span>
            <Kbd>→</Kbd>
          </span>
        </button>
      )}
      {hint !== null && (
        <span className="sr-only" id={id("ready-hint-more")}>
          {hint.label}
        </span>
      )}
    </div>
  );
}
