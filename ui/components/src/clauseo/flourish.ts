// The motion that decorates the live room. Every state lands first, in React; these only animate
// what has already landed, and only when motion is allowed (full render, full motion). Each animation is finished by
// a timer once its time has passed, never by its own promise, so a hidden or busy page is never left halfway and
// nothing is left running at rest. One per room: it finds what it moves through the room's own nodes.
import { motionAllowed } from "../lib/display.ts";
import type { HandStore } from "./hand.ts";
import { isWide } from "./layout.ts";
import { dur, ease } from "./motion.ts";
import type { RoomNodes } from "./room-nodes.ts";

export type Flourish = {
  /** One WAAPI animation, with a timer that finishes it once its delay and duration have passed. */
  move: (
    node: Element | null | undefined,
    frames: Keyframe[],
    opts: KeyframeAnimationOptions,
  ) => Animation | null;
  /** Stops every timer this room's motion started, and lands what it was moving: a Replay starts clean. */
  stopAll: () => void;
  /** As the answer cites a paper or a judgment, its tile or row comes forward in its colour for about 340 ms, then
   * settles back into the wall, lit. On the wide window only, and never while a card is open. No line is drawn. */
  flash: (last: string) => void;
  /** A new row arriving: it slides 8 px up and comes in over 220 ms. */
  arrive: (node: Element | null) => void;
  /** A check finishing: Good law scales in from 0.9 and its tick draws itself. */
  settleCheck: (id: string) => void;
  /** The question leaving the composer and landing in the thread, as one element, on the spring. from: where the
   * field's words stood. */
  fly: (from: DOMRect | null) => void;
  /** The answer done: the meta line's chip comes in and its tick draws itself. */
  metaDone: () => void;
  /** The prepared draft: its tile drops 18 px into the wall and settles with weight; the Ready card follows 60 ms
   * behind. draftId: the draft's paper id, whose tile drops. */
  drop: (draftId: string) => void;
  /** The other side's plane arriving: it slides 8 px and stops. */
  arriveOtherSide: () => void;
};

/** The first element child of a kind inside a node: a tick's one path, a chip's icon. */
function firstOf<T extends Element>(
  node: Element | null | undefined,
  kind: abstract new () => T,
): T | null {
  if (node == null) return null;
  const walk = node.ownerDocument.createTreeWalker(node, NodeFilter.SHOW_ELEMENT);
  for (let n = walk.nextNode(); n !== null; n = walk.nextNode()) if (n instanceof kind) return n;
  return null;
}

export function createFlourish(nodes: RoomNodes, hand: HandStore): Flourish {
  // each timer, with what it does when it fires: stopping early does it at once
  const live = new Map<number, () => void>();
  let flashNode: Element | null = null;
  let flashTimer = 0;

  const move: Flourish["move"] = (node, frames, opts) => {
    if (node == null || !motionAllowed() || typeof node.animate !== "function") return null;
    let a: Animation;
    try {
      a = node.animate(frames, opts);
    } catch {
      return null;
    }
    const ms = Number(opts.delay ?? 0) + Number(opts.duration ?? 0) + 120;
    const done = () => land(a);
    const id = window.setTimeout(() => {
      live.delete(id);
      done();
    }, ms);
    live.set(id, done);
    return a;
  };

  /** A tick that draws itself: the check's one path, from nothing to whole. */
  const drawTick = (icon: Element | null | undefined, delay = 0) => {
    const path = firstOf(icon, SVGPathElement);
    if (path === null || !motionAllowed()) return;
    path.setAttribute("pathLength", "1");
    path.style.strokeDasharray = "1";
    const d = dur("arrive");
    move(path, [{ strokeDashoffset: 1 }, { strokeDashoffset: 0 }], {
      duration: d,
      delay,
      easing: ease("arrive"),
      fill: "backwards",
    });
    const done = () => {
      path.style.strokeDasharray = "";
      path.removeAttribute("pathLength");
    };
    const id = window.setTimeout(
      () => {
        live.delete(id);
        done();
      },
      d + delay + 40,
    );
    live.set(id, done);
  };

  const arrive: Flourish["arrive"] = (node) => {
    move(
      node,
      [
        { transform: "translateY(8px)", opacity: 0 },
        { transform: "none", opacity: 1 },
      ],
      {
        duration: dur("arrive"),
        easing: ease("arrive"),
      },
    );
  };

  return {
    move,
    stopAll: () => {
      for (const [id, done] of live) {
        window.clearTimeout(id);
        done();
      }
      live.clear();
      window.clearTimeout(flashTimer);
      flashNode?.removeAttribute("data-flash");
      flashNode = null;
    },
    flash: (last) => {
      window.clearTimeout(flashTimer);
      flashNode?.removeAttribute("data-flash");
      flashNode = null;
      if (!motionAllowed() || !isWide() || document.hidden || hand.get().state !== "none") return;
      const [kind, id = ""] = last.split(":");
      const node = kind === "paper" ? nodes.find("tile", id) : nodes.find("row", id);
      if (node === null) return;
      node.setAttribute("data-flash", "");
      flashNode = node;
      flashTimer = window.setTimeout(() => {
        node.removeAttribute("data-flash");
        if (flashNode === node) flashNode = null;
      }, dur("cite"));
    },
    arrive,
    settleCheck: (id) => {
      const chip = nodes.find("check", id)?.firstElementChild;
      move(chip, [{ transform: "scale(0.9)" }, { transform: "none" }], {
        duration: dur("settle"),
        easing: ease("spring"),
      });
      drawTick(firstOf(chip, SVGSVGElement), 40);
    },
    fly: (from) => {
      const asked = nodes.get("asked");
      const who = nodes.get("askedWho");
      if (from === null || asked === null) return;
      const to = asked.getBoundingClientRect();
      const dx = from.left - to.left;
      const dy = from.top - to.top;
      const d = dur("fly");
      move(asked, [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], {
        duration: d,
        easing: ease("spring"),
      });
      move(who, [{ transform: `translateY(${Math.round(dy * 0.5)}px)` }, { transform: "none" }], {
        duration: d,
        easing: ease("spring"),
      });
    },
    metaDone: () => {
      const chip = nodes.get("workDone");
      move(
        chip,
        [
          { opacity: 0, transform: "translateY(4px)" },
          { opacity: 1, transform: "none" },
        ],
        {
          duration: dur("arrive"),
          easing: ease("arrive"),
        },
      );
      drawTick(chip?.firstElementChild, 80);
    },
    drop: (draftId) => {
      const d = dur("drop");
      move(
        nodes.find("face", draftId),
        [{ transform: "translateY(-18px)" }, { transform: "none" }],
        {
          duration: d,
          easing: ease("spring"),
        },
      );
      move(
        nodes.get("ready"),
        [
          { transform: "translateY(-10px)", opacity: 0 },
          { transform: "none", opacity: 1 },
        ],
        {
          duration: d,
          delay: 60,
          easing: ease("spring"),
          fill: "backwards",
        },
      );
    },
    arriveOtherSide: () => arrive(nodes.get("otherside")),
  };
}

/** Lands an animation where it was going: finished, or cancelled where it can't finish. */
function land(a: Animation) {
  if (a.playState === "finished" || a.playState === "idle") return;
  try {
    a.finish();
  } catch {
    a.cancel();
  }
}
