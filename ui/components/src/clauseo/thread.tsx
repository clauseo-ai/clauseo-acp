import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
import { useDisplay } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { isWide } from "./layout.ts";
import { dur, ease } from "./motion.ts";
import { useRoom, type RoomNodes } from "./room-scope.ts";
import { Say } from "./say.tsx";

type Anchor = { i: number; frac: number; off: number };

/** The pinned question's spoken end, "Scroll back to it", taken from the template after the question's slot, so
 * its name reads the visible words first. */
const scrollBack = (template: string) => template.slice(template.indexOf("\u0000") + 1).trim();

/** What the reader's place is kept by, in the page's order: each exchange, and within the answer its meta line,
 * each block, the accept row and the Ready card. */
function anchorables(inner: Element, nodes: RoomNodes): Element[] {
  const out: Element[] = [];
  const step = nodes.get("answerStep");
  for (const child of inner.children) {
    out.push(child);
    if (child !== step) continue;
    const answer = nodes.get("answer");
    const parts = [
      nodes.get("meta"),
      ...(answer === null ? [] : answer.children),
      nodes.get("acceptRow"),
      nodes.get("ready"),
    ];
    for (const part of parts) if (part != null && step.contains(part)) out.push(part);
  }
  return out;
}

/** How far below the window's top the question rests on a narrow window: 12 px, and a phone's status bar. The
 * stylesheet gives it as the question's scroll margin, where env(safe-area-inset-top) is resolved. */
const clearance = (q: Element) => Number.parseFloat(getComputedStyle(q).scrollMarginTop) || 12;

/** The thread and, over it, the question pinned once it has scrolled away. At rest the view shows the answer from
 * its first line, just under the pinned question; on a narrow window the page holds the question at the top. A
 * change of language keeps the reader's place. While the answer streams (live), the view follows it without
 * moving: each time the words take a new line, the scroll is set once, before the frame paints, to the end of the
 * stream but never past the answer's first line. A lawyer who scrolls takes the view over. */
export function Thread({
  question,
  who,
  children,
  live = false,
  resting = true,
}: {
  question: string;
  who: string;
  children: ReactNode;
  /** The answer is streaming: follow it. */
  live?: boolean;
  /** The answer has settled: land where the view rests. */
  resting?: boolean;
}) {
  const w = useWords();
  const { lang, textSize } = useDisplay();
  // the room's reach never changes while the thread stands, so the measuring below reads it as it reads its refs
  const scope = useRoom();
  const reach = useRef(scope);
  const thread = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const pin = useRef<HTMLButtonElement>(null);
  // the pin is shown and hidden on the element itself, not through React state, so a scroll never costs a commit:
  // while the answer streams the room commits at most once a frame, and that commit is the stream's
  const pinned = useRef(false);
  const setPinned = (show: boolean) => {
    const n = pin.current;
    if (n === null || pinned.current === show) return;
    pinned.current = show;
    n.hidden = !show;
    // it slides in from above over 200 ms; under reduced motion and in lite it's simply there
    if (show)
      reach.current.motion.move(n, [{ transform: "translateY(-100%)" }, { transform: "none" }], {
        duration: dur("state"),
        easing: ease("arrive"),
      });
  };
  const anchor = useRef<Anchor | null>(null);
  const userScrolled = useRef(false);

  const pinHeight = () => {
    const n = pin.current;
    if (n === null) return 0;
    if (!n.hidden) return n.offsetHeight;
    n.style.visibility = "hidden";
    n.hidden = false;
    const h = n.offsetHeight;
    n.hidden = true;
    n.style.visibility = "";
    return h;
  };

  const check = () => {
    const t = thread.current;
    const q = reach.current.nodes.get("asked");
    if (t == null || q == null) return;
    const scrolls = getComputedStyle(t).overflowY !== "visible";
    const centre = reach.current.nodes.get("centre");
    centre?.toggleAttribute(
      "data-more",
      scrolls && t.scrollHeight - t.scrollTop - t.clientHeight > 2,
    );
    const r = q.getBoundingClientRect();
    const tr = t.getBoundingClientRect();
    setPinned(scrolls ? r.bottom < tr.top + pinHeight() : r.bottom < 4 && tr.bottom > 160);
  };

  const viewTop = () => {
    const t = thread.current;
    const top = t === null || !isWide() ? 0 : t.getBoundingClientRect().top;
    return top + (pin.current?.hidden === false ? pin.current.offsetHeight : 0);
  };
  const readAnchor = () => {
    const i0 = inner.current;
    if (i0 === null) return;
    const top = viewTop();
    const list = anchorables(i0, reach.current.nodes);
    for (let i = 0; i < list.length; i++) {
      const r = list[i]?.getBoundingClientRect();
      if (r !== undefined && r.bottom > top + 1) {
        anchor.current = {
          i,
          frac: r.top < top ? (top - r.top) / Math.max(1, r.height) : 0,
          off: Math.max(0, r.top - top),
        };
        return;
      }
    }
  };
  const keepAnchor = () => {
    const a = anchor.current;
    const i0 = inner.current;
    const t = thread.current;
    if (a === null || i0 === null || t === null) return;
    const node = anchorables(i0, reach.current.nodes)[a.i];
    if (node === undefined) return;
    const r = node.getBoundingClientRect();
    const by = r.top + a.frac * r.height - (viewTop() + a.off);
    if (isWide()) t.scrollTop = Math.round(t.scrollTop + by);
    else window.scrollTo(0, Math.round(window.scrollY + by));
  };

  // where the view rests: the answer's first line under the pinned question
  const rest = () => {
    const t = thread.current;
    const i0 = inner.current;
    if (t === null || i0 === null || userScrolled.current) return;
    if (isWide()) {
      const step = reach.current.nodes.get("answerStep");
      const end = t.scrollHeight - t.clientHeight;
      if (step !== null) t.scrollTop = Math.max(0, Math.min(end, step.offsetTop - pinHeight() - 4));
    } else {
      const q = reach.current.nodes.get("questionStep");
      if (q !== null)
        window.scrollTo(
          0,
          Math.max(0, Math.round(window.scrollY + q.getBoundingClientRect().top - clearance(q))),
        );
    }
    check();
    readAnchor();
  };

  useLayoutEffect(() => {
    if (!resting) return;
    rest();
    let on = true;
    void document.fonts.ready.then(() => {
      if (on) rest();
    });
    return () => {
      on = false;
    };
    // once, as the answer settles; the fonts settle it again
  }, [resting]);

  // following the stream: the end of the thread, but never past the answer's first line; on a narrow window the
  // page holds the question at the top. Set directly, never glided, whenever the thread's height changes, which a
  // ResizeObserver reports after layout and before paint, so no frame shows the words below the fold.
  useEffect(() => {
    const t = thread.current;
    const i0 = inner.current;
    if (!live || t === null || i0 === null) return;
    const follow = () => {
      if (userScrolled.current) return;
      if (isWide()) {
        const end = t.scrollHeight - t.clientHeight;
        const step = reach.current.nodes.get("answerStep");
        const goal = step === null ? end : Math.min(end, step.offsetTop - pinHeight() - 4);
        const next = Math.max(0, Math.round(goal));
        if (Math.abs(t.scrollTop - next) >= 1) t.scrollTop = next;
      } else {
        const q = reach.current.nodes.get("questionStep");
        if (q === null) return;
        const by = Math.round(q.getBoundingClientRect().top - clearance(q));
        // the page scrolls smoothly for the lawyer's own jumps; the follow never glides
        if (Math.abs(by) >= 1)
          window.scrollTo({ top: Math.max(0, window.scrollY + by), behavior: "instant" });
      }
    };
    const ro = new ResizeObserver(follow);
    ro.observe(i0);
    ro.observe(t);
    follow();
    return () => ro.disconnect();
  }, [live]);

  // a change of language or size keeps the reader's place
  const first = useRef(true);
  useLayoutEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    keepAnchor();
    check();
  }, [lang, textSize]);

  useEffect(() => {
    const t = thread.current;
    if (t === null) return;
    let raf = 0;
    const onScroll = () => {
      if (raf !== 0) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        check();
        readAnchor();
      });
    };
    const mine = () => {
      userScrolled.current = true;
    };
    t.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    t.addEventListener("wheel", mine, { passive: true });
    t.addEventListener("touchmove", mine, { passive: true });
    window.addEventListener("wheel", mine, { passive: true });
    return () => {
      cancelAnimationFrame(raf);
      t.removeEventListener("scroll", onScroll);
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      t.removeEventListener("wheel", mine);
      t.removeEventListener("touchmove", mine);
      window.removeEventListener("wheel", mine);
    };
  }, []);

  const backToQuestion = () => {
    const q = reach.current.nodes.get("questionStep");
    const t = thread.current;
    if (q === null || t === null) return;
    userScrolled.current = true;
    if (!isWide()) window.scrollBy({ top: q.getBoundingClientRect().top - 72 });
    else t.scrollTop = Math.max(0, q.offsetTop - 16);
    check();
  };

  return (
    <>
      <button
        type="button"
        ref={(node) => {
          pin.current = node;
          return scope.nodes.slot("pin")(node);
        }}
        className="room-pin"
        hidden
        onClick={backToQuestion}
      >
        <span className="room-pin-line">
          <span className="room-pin-who">{who}</span>
          <span className="room-pin-text">
            <Say>{question}</Say>
          </span>
          <span className="sr-only">
            {scrollBack(w.t("Your question: {q} Scroll back to it", { q: "\u0000" }))}
          </span>
        </span>
      </button>
      <div
        className="room-thread"
        ref={(node) => {
          thread.current = node;
          return scope.nodes.slot("thread")(node);
        }}
        role="log"
        aria-label={w.t("This matter's thread")}
      >
        <div
          className="room-thread-inner"
          ref={(node) => {
            inner.current = node;
            return scope.nodes.slot("threadInner")(node);
          }}
        >
          {children}
        </div>
      </div>
    </>
  );
}
