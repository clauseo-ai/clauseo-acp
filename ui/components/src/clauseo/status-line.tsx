// The status line on the composer's top edge: how long the answer took, the plan the questions go to, and the
// prepared draft's hint while its Ready card is out of view.
import { useLayoutEffect, useRef, type PointerEvent } from "react";
import { Select as BaseSelect } from "@base-ui/react/select";
import { Kbd } from "../ui/kbd.tsx";
import { SegmentedSwitch } from "../ui/segmented.tsx";
import { useLayer } from "../ui/layer-context.ts";
import { raised, row } from "../ui/recipes-overlays.ts";
import { IconGoodLaw } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { motionAllowed, useDisplay } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { ui } from "../lib/words-ui.ts";
import { useThreadWords } from "../lib/words-thread.ts";
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

/** "Uses your Claude plan" on the status line, which opens the plan to ask with. At rest it is the status line's
 * own words, unchanged; under the pointer or focused it takes a quiet fill. */
function PlanTrigger<V extends string>({ plan, choice }: { plan: string; choice: PlanChoice<V> }) {
  const w = useThreadWords();
  const layer = useLayer();
  return (
    <BaseSelect.Root
      items={choice.options}
      value={choice.value}
      onValueChange={(next) => {
        const chosen = choice.options.find((o) => o.value === next);
        if (chosen !== undefined) choice.onValueChange(chosen.value);
      }}
    >
      <BaseSelect.Trigger
        className="room-status-plan room-plan"
        aria-label={`${plan}. ${w.ui("Plan to ask with")}`}
      >
        <Say>{plan}</Say>
      </BaseSelect.Trigger>
      <BaseSelect.Portal container={layer}>
        <BaseSelect.Positioner
          alignItemWithTrigger={false}
          positionMethod="fixed"
          side="top"
          align="start"
          sideOffset={6}
          collisionPadding={8}
          className="z-20"
        >
          <BaseSelect.Popup className={raised({ width: "select" })}>
            <BaseSelect.List>
              {choice.options.map((o) => (
                <BaseSelect.Item
                  key={o.value}
                  value={o.value}
                  label={o.label}
                  className={cn(row, "justify-between")}
                >
                  <BaseSelect.ItemText className="deva-nudge">{o.label}</BaseSelect.ItemText>
                  <BaseSelect.ItemIndicator className="flex-none text-primary">
                    <IconGoodLaw size={16} />
                  </BaseSelect.ItemIndicator>
                </BaseSelect.Item>
              ))}
            </BaseSelect.List>
          </BaseSelect.Popup>
        </BaseSelect.Positioner>
      </BaseSelect.Portal>
    </BaseSelect.Root>
  );
}

/** The status line on the composer's top edge: "Took 38 seconds", a dot, "Uses your Claude plan", and the ready hint
 * at its right while the Ready card is out of view. Before the first answer it holds the plan alone. It never
 * truncates: the hint gives up its words first and keeps its key cap; if that still isn't enough the line wraps to
 * two. When the working line closes into it, the time comes up into place. */
export function StatusLine<V extends string>({
  took,
  plan,
  hint,
  choice,
  hidden = false,
}: {
  took: string | null;
  plan: string;
  hint: ReadyHint | null;
  choice?: PlanChoice<V> | undefined;
  /** While the working line stands in its place. It stays mounted, so the plan's select keeps its state. */
  hidden?: boolean;
}) {
  const w = useWords();
  const { lang, textSize } = useDisplay();
  const wide = useWide();
  const box = useRef<HTMLDivElement>(null);
  const line = useRef<HTMLParagraphElement>(null);
  const tookRef = useRef<HTMLSpanElement>(null);
  const goRef = useRef<HTMLSpanElement>(null);
  const hadTook = useRef(took !== null);
  const { id, hand } = useRoom();
  const hintState = hint?.state ?? "none";

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
  }, [lang, textSize, wide, hintState, took, hidden]);

  const hold = (on: boolean) => (e?: PointerEvent) => {
    if (e?.pointerType !== "touch") hand.holdDraft("hint", on);
  };
  return (
    <div className="room-status" ref={box} hidden={hidden}>
      <p className="room-status-line" ref={line} role="status">
        {took !== null && (
          <span className="room-status-took" ref={tookRef}>
            <b>{took}</b>
            <i className="room-dot" aria-hidden="true" />
          </span>
        )}
        {choice === undefined ? (
          <span className="room-status-plan">
            <Say>{plan}</Say>
          </span>
        ) : choice.segmented !== undefined ? (
          <span className="room-status-plan room-status-seg">
            <span className="deva-nudge">{ui(lang, "Plan to ask with")}</span>
            <SegmentedSwitch<V>
              label={ui(lang, "Plan to ask with")}
              value={choice.value}
              options={choice.segmented}
              onValueChange={choice.onValueChange}
              className="room-plan-seg"
            />
          </span>
        ) : (
          <PlanTrigger plan={plan} choice={choice} />
        )}
      </p>
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
