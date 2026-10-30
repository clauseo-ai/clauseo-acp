import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
import { Button } from "../ui/button.tsx";
import { IconAccept, IconGoodLaw } from "../icons/icons.tsx";
import { motionAllowed, useDisplay } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { curve, hold, release, setWidths, settle } from "./ink.ts";
import type { AcceptState } from "./logic.ts";
import { dur, ease } from "./motion.ts";
import type { RoomNodes } from "./room-nodes.ts";
import { useRoom } from "./room-scope.ts";

export type AcceptProps = {
  state: AcceptState;
  words: { action: string; pending: string; done: string; undo: string };
  onAccept: () => void;
  onUndo: () => void;
};

/** The answer's foot. "Not yet accepted", with "Accept into the matter" beside it, which fades in when the answer is
 * hovered or focused and never moves a line; on touch it shows. Accepted, the mark reads "Accepted 4:33 pm" with a
 * check, at the lawyer's weight, and a quiet Undo stands beside it. The toolbar's Undo does the same. */
export function Accept({ state, words, onAccept, onUndo }: AcceptProps) {
  const w = useWords();
  const { nodes } = useRoom();
  const mark = useRef<HTMLSpanElement>(null);
  const accept = useRef<HTMLButtonElement>(null);
  const undo = useRef<HTMLButtonElement>(null);
  const was = useRef(state);
  const focusNext = useRef(false);

  useLayoutEffect(() => {
    if (was.current === state) return;
    was.current = state;
    if (state === "accepted" && motionAllowed()) {
      mark.current?.animate(
        [
          { opacity: 0, transform: "translateY(3px)" },
          { opacity: 1, transform: "none" },
        ],
        {
          duration: dur("state"),
          easing: ease("arrive"),
        },
      );
    }
    if (focusNext.current) {
      focusNext.current = false;
      (state === "accepted" ? undo : accept).current?.focus({ preventScroll: true });
    }
  }, [state]);

  return (
    <div
      className="accept-row"
      ref={nodes.slot("acceptRow")}
      role="group"
      aria-label={w.t("This answer")}
      data-state={state}
    >
      <span className="accept-mark" role="status" ref={mark}>
        {state === "accepted" && <IconGoodLaw size={14} />}
        <span>{state === "accepted" ? words.done : words.pending}</span>
      </span>
      {state === "pending" ? (
        <Button
          ref={accept}
          kind="tone"
          size="small"
          className="accept-btn"
          onClick={() => {
            focusNext.current = true;
            onAccept();
          }}
        >
          <IconAccept size={16} />
          {words.action}
        </Button>
      ) : (
        <Button
          ref={undo}
          kind="quiet"
          size="small"
          className="accept-undo"
          aria-label={w.t("Undo accepting this answer")}
          onClick={() => {
            focusNext.current = true;
            onUndo();
          }}
        >
          {words.undo}
        </Button>
      )}
    </div>
  );
}

/** The lines held where they stand at Clauseo's weight, and each block given its own width at the lawyer's. */
function setAccepted(nodes: RoomNodes, node: HTMLElement) {
  release(node);
  node.dataset["accept"] = "pending";
  setWidths(nodes.blocksIn(node));
  hold(node, nodes.blocksIn(node));
  node.dataset["accept"] = "accepted";
}

/** The answer's words, keyed by the language where it's used, which take the lawyer's weight once accepted: 400 to
 * 600 in 280 ms, each weight paired with the width measured for its block and script, the line breaks held, so
 * nothing rewraps. */
export function AcceptedAnswer({ state, children }: { state: AcceptState; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const was = useRef<AcceptState | null>(null);
  const { lang, textSize } = useDisplay();
  const settleTimer = useRef(0);
  const { nodes } = useRoom();

  useLayoutEffect(() => {
    const node = ref.current;
    if (node === null) return;
    const before = was.current;
    was.current = state;
    for (const a of node.getAnimations({ subtree: true })) a.cancel();
    window.clearTimeout(settleTimer.current);
    if (state === "accepted") {
      if (before !== "accepted") {
        // measured at Clauseo's weight, before the weight moves
        setAccepted(nodes, node);
        if (before === "pending" && motionAllowed()) settle(nodes.blocksIn(node), true);
      }
      return;
    }
    node.dataset["accept"] = "pending";
    if (before === "accepted" && motionAllowed()) {
      settle(nodes.blocksIn(node), false);
      settleTimer.current = window.setTimeout(() => release(node), dur("accept") + 40);
    } else release(node);
  }, [state, nodes]);
  // leaving the page, the settle's last step goes with it
  useEffect(() => {
    const timer = settleTimer;
    return () => window.clearTimeout(timer.current);
  }, []);

  // the size changes: the lines are set afresh, and the widths fit the new cut. (A change of language remounts the
  // answer, keyed by it, and the first effect sets it afresh.)
  useLayoutEffect(() => {
    const node = ref.current;
    if (node === null || was.current !== "accepted") return;
    setAccepted(nodes, node);
  }, [textSize, nodes]);

  // a window that changes width sets the lines afresh too
  useEffect(() => {
    const node = ref.current;
    if (node === null) return;
    let width = node.clientWidth;
    const ro = new ResizeObserver(() => {
      if (node.clientWidth === width) return;
      width = node.clientWidth;
      if (was.current !== "accepted") return;
      setAccepted(nodes, node);
    });
    ro.observe(node);
    return () => ro.disconnect();
  }, [nodes]);

  // worked out ahead, while nothing else is happening, so Accept responds at once
  useEffect(() => {
    const node = ref.current;
    if (node === null) return;
    let pending = 0;
    const idle = (fn: () => void) => {
      pending =
        typeof window.requestIdleCallback === "function"
          ? window.requestIdleCallback(fn, { timeout: 1500 })
          : window.setTimeout(fn, 200);
    };
    let i = 0;
    let stop = false;
    const next = () =>
      idle(() => {
        const list = nodes.blocksIn(node);
        if (stop || i >= list.length || node.dataset["accept"] === "accepted") return;
        const b = list[i++];
        if (b !== undefined) curve(b);
        next();
      });
    next();
    return () => {
      stop = true;
      if (typeof window.cancelIdleCallback === "function") window.cancelIdleCallback(pending);
      else window.clearTimeout(pending);
    };
  }, [lang, nodes]);

  return (
    <div
      className="answer"
      ref={(node) => {
        ref.current = node;
        return nodes.slot("answer")(node);
      }}
    >
      {children}
    </div>
  );
}
