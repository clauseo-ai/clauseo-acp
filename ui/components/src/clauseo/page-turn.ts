// The page turn. The prepared draft's tile lifts in full yellow and a sheet grows out of it,
// clipped from the tile's rectangle to the page's, across the gap and onto the work, paling from yellow to the sheet
// tone, while the answer recedes beneath it. Back runs it in reverse. The state lands first and the motion decorates
// it; timers, never an animation's promise, settle what follows.
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { flushSync } from "react-dom";
import { motionAllowed } from "../lib/display.ts";
import { isWide } from "./layout.ts";
import { dur, ease } from "./motion.ts";
import { useRoom } from "./room-scope.ts";

/** answer: the thread in view. opening: the sheet in flight toward the work. page: the draft's first page as the work.
 * closing: the sheet in flight back into its tile. */
export type TurnState = "answer" | "opening" | "page" | "closing";

const spring = () => ease("spring");
const arrive = () => ease("arrive");

type Box = { left: number; top: number; right: number; bottom: number };

export type PageTurnRefs = {
  page: RefObject<HTMLElement | null>;
  turn: RefObject<HTMLElement | null>;
  back: RefObject<HTMLButtonElement | null>;
  ready: RefObject<HTMLButtonElement | null>;
};

/** The page turn's state, and show and hide, over the elements it moves. draftId: the prepared draft's paper id,
 * whose tile the sheet grows out of. */
export function usePageTurn(refs: PageTurnRefs, draftId: string) {
  const { hand, nodes } = useRoom();
  const [state, setState] = useState<TurnState>("answer");
  const [opened, setOpened] = useState(false);
  const keep = useRef(0);
  const timers = useRef<number[]>([]);
  const away = useRef<Animation | null>(null);
  const current = useRef<TurnState>("answer");
  useLayoutEffect(() => {
    current.current = state;
  }, [state]);

  const later = (fn: () => void, ms: number) => {
    timers.current.push(window.setTimeout(fn, ms));
  };
  const clearTimers = () => {
    for (const t of timers.current) window.clearTimeout(t);
    timers.current = [];
  };
  useEffect(() => clearTimers, []);

  const thread = () => nodes.get("thread");
  const inner = () => nodes.get("threadInner");
  const face = () => nodes.find("face", draftId);
  const pageBox = (): Box | null => {
    const p = refs.page.current?.getBoundingClientRect();
    const v = nodes.get("turnScroll")?.getBoundingClientRect();
    if (p === undefined || v === undefined) return null;
    return {
      left: p.left,
      right: p.right,
      top: Math.max(p.top, v.top),
      bottom: Math.min(p.bottom, v.bottom),
    };
  };

  /** The sheet in flight: one fixed box spanning both ends, clipped from one rectangle to the other. */
  const fly = (from: Box, to: Box, back: boolean): number => {
    const g = nodes.get("ghost");
    if (g === null) return 0;
    const L = Math.min(from.left, to.left);
    const T = Math.min(from.top, to.top);
    const R = Math.max(from.right, to.right);
    const B = Math.max(from.bottom, to.bottom);
    g.style.left = `${L}px`;
    g.style.top = `${T}px`;
    g.style.width = `${R - L}px`;
    g.style.height = `${B - T}px`;
    const ins = (r: Box, rad: number) =>
      `inset(${Math.round(r.top - T)}px ${Math.round(R - r.right)}px ${Math.round(B - r.bottom)}px ${Math.round(r.left - L)}px round ${rad}px)`;
    g.style.clipPath = ins(to, back ? 10 : 4);
    g.hidden = false;
    const ms = dur(back ? "turn-back" : "turn");
    const a1 = g.animate(
      [{ clipPath: ins(from, back ? 4 : 10) }, { clipPath: ins(to, back ? 10 : 4) }],
      {
        duration: ms,
        easing: back ? arrive() : spring(),
      },
    );
    const y = g.firstElementChild;
    const a2 = y?.animate(
      back
        ? [{ opacity: 0 }, { opacity: 0.85, offset: 0.6 }, { opacity: 1 }]
        : [
            { opacity: 1 },
            { opacity: 0.85, offset: 0.15 },
            { opacity: 0, offset: 0.5 },
            { opacity: 0 },
          ],
      { duration: ms, easing: "linear", fill: "both" },
    );
    // arriving, the sheet gives way to the page beneath as its words come up; leaving, it takes over from the page
    const a3 = g.animate(
      back
        ? [{ opacity: 0 }, { opacity: 1, offset: 0.2 }, { opacity: 1 }]
        : [{ opacity: 1 }, { opacity: 1, offset: 0.62 }, { opacity: 0 }],
      { duration: ms, easing: "linear", fill: "both" },
    );
    later(() => {
      for (const a of [a1, a2, a3]) a?.cancel();
      g.hidden = true;
    }, ms + 20);
    return ms;
  };

  const show = () => {
    if (current.current === "page" || current.current === "opening") return;
    clearTimers();
    hand.put();
    const wide = isWide();
    const tile = face();
    // measured at rest, before anything moves
    const from = tile !== null && wide ? tile.getBoundingClientRect() : null;
    const t = thread();
    keep.current = t?.scrollTop ?? 0;
    // the turned page stands clear of the composer: its height, on this room's window alone
    const composer = nodes.get("composer");
    nodes.get("window")?.style.setProperty("--composer-h", `${composer?.offsetHeight ?? 140}px`);
    current.current = "opening";
    flushSync(() => {
      setState("opening");
      setOpened(true);
    });
    hand.holdDraft("turn", true);
    if (!wide && refs.turn.current !== null) {
      window.scrollTo(
        0,
        Math.max(0, refs.turn.current.getBoundingClientRect().top + window.scrollY - 8),
      );
    }
    refs.back.current?.focus({ preventScroll: true });
    const done = () => {
      current.current = "page";
      setState("page");
      if (t !== null) t.inert = true;
    };
    const to = pageBox();
    if (!motionAllowed() || !wide || from === null || to === null) {
      done();
      if (!wide && motionAllowed()) {
        refs.page.current?.animate(
          [
            { opacity: 0, transform: "translateY(12px)" },
            { opacity: 1, transform: "none" },
          ],
          {
            duration: dur("settle"),
            easing: spring(),
          },
        );
      }
      return;
    }
    const flight = fly(from, to, false);
    away.current?.cancel();
    away.current =
      inner()?.animate(
        [
          { transform: "none", opacity: 1 },
          { transform: "scale(0.97)", opacity: 0 },
        ],
        {
          duration: dur("grow"),
          easing: arrive(),
          fill: "forwards",
        },
      ) ?? null;
    refs.turn.current?.animate([{ opacity: 0 }, { opacity: 1 }], {
      duration: dur("state"),
      easing: arrive(),
    });
    // the page's own words come up as the sheet finishes opening
    refs.page.current?.animate([{ opacity: 0 }, { opacity: 0, offset: 0.62 }, { opacity: 1 }], {
      duration: flight,
      easing: "linear",
    });
    later(done, dur("settle"));
  };

  const hide = () => {
    if (current.current !== "page" && current.current !== "opening") return;
    clearTimers();
    nodes.get("ghost")?.setAttribute("hidden", "");
    const wide = isWide();
    const tile = face();
    // measured while the page is still up and the tile still lifted
    const from = wide && motionAllowed() ? pageBox() : null;
    const to = from !== null && tile !== null ? tile.getBoundingClientRect() : null;
    const t = thread();
    if (t !== null) t.inert = false;
    current.current = "closing";
    flushSync(() => setState("closing"));
    away.current?.cancel();
    away.current = null;
    if (t !== null) t.scrollTop = keep.current;
    refs.ready.current?.focus({ preventScroll: true });
    const close = () => {
      current.current = "answer";
      setState("answer");
    };
    if (from === null || to === null) {
      close();
      hand.holdDraft("turn", false);
      if (!wide) nodes.get("ready")?.scrollIntoView({ block: "center" });
      return;
    }
    const page = refs.page.current;
    const turn = refs.turn.current;
    const a = page?.animate([{ opacity: 1 }, { opacity: 0 }], {
      duration: dur("leave"),
      easing: "linear",
      fill: "forwards",
    });
    const b = turn?.animate([{ opacity: 1 }, { opacity: 0 }], {
      duration: dur("state"),
      delay: 40,
      easing: arrive(),
      fill: "forwards",
    });
    const flight = fly(from, to, true);
    inner()?.animate(
      [
        { transform: "scale(0.97)", opacity: 0 },
        { transform: "none", opacity: 1 },
      ],
      {
        duration: dur("drop"),
        delay: 60,
        easing: spring(),
        fill: "backwards",
      },
    );
    later(() => {
      close();
      a?.cancel();
      b?.cancel();
    }, flight - 50);
    // the tile settles back into the wall once the sheet is home
    later(() => hand.holdDraft("turn", false), flight - 20);
  };

  return { state, opened, show, hide };
}
