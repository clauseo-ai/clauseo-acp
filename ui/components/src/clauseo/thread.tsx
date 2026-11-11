import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { IconArrowDown } from "../icons/icons.tsx";
import { motionAllowed, useDisplay } from "../lib/display.ts";
import { langOfText } from "../lib/script.ts";
import { th } from "../lib/words-thread.ts";
import { useWords } from "../lib/words.ts";
import { Button } from "../ui/button.tsx";
import { createFollow, type FollowPlace } from "./follow.ts";
import { isWide } from "./layout.ts";
import { dur, ease } from "./motion.ts";
import type { Flourish } from "./flourish.ts";
import { useRoom, type RoomNodes } from "./room-scope.ts";
import { Say } from "./say.tsx";
import { startOf } from "./question-fold-logic.ts";

/** The pinned question's words: its start, more than one line holds at any width, so a 6 MiB question costs the bar
 * no more layout than a short one; the line's ellipsis (thread.css) shows where it is cut. */
const PIN_CHARS = 240;

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

/** Where the thread's end is (follow.ts): the thread's own on a wide window; under 1000 px the page scrolls, and the
 * thread's end is where the docked composer starts. */
function threadPlace(nodes: RoomNodes): FollowPlace {
  const place: FollowPlace = {
    gap: () => {
      const t = nodes.get("thread");
      if (t === null) return 0;
      if (isWide()) return t.scrollHeight - t.scrollTop - t.clientHeight;
      const top = nodes.get("composer")?.getBoundingClientRect().top ?? Infinity;
      return t.getBoundingClientRect().bottom - Math.min(window.innerHeight, top);
    },
    top: () => (isWide() ? (nodes.get("thread")?.scrollTop ?? 0) : window.scrollY),
    toEnd: (smooth) => {
      const t = nodes.get("thread");
      if (t === null) return;
      const behavior = smooth && motionAllowed() ? "smooth" : "instant";
      if (!isWide()) window.scrollTo({ top: Math.max(0, window.scrollY + place.gap()), behavior });
      else if (behavior === "smooth") t.scrollTo({ top: t.scrollHeight, behavior });
      else t.scrollTop = t.scrollHeight;
    },
  };
  return place;
}

/** Shows or hides Latest's dock; it rises 8 px into place as it shows, where motion is allowed. */
function showLatest(
  scope: { nodes: RoomNodes; motion: { move: Flourish["move"] } },
  show: boolean,
) {
  const n = scope.nodes.get("latest");
  if (n === null || n.hidden === !show) return;
  n.hidden = !show;
  const button = n.firstElementChild;
  if (show && button instanceof HTMLElement)
    scope.motion.move(
      button,
      [
        { opacity: 0, transform: "translateY(8px)" },
        { opacity: 1, transform: "none" },
      ],
      {
        duration: dur("state"),
        easing: ease("arrive"),
      },
    );
}

/** Keys that move a scroller up. */
const UP = new Set(["ArrowUp", "PageUp", "Home"]);

/** The thread and, over it, the question pinned once it has scrolled away. The view keeps to the thread's end while
 * the lawyer is there (follow.ts): it opens at the end, and whatever comes in, the answer's words as they stream
 * included, keeps it there; the scroll is set once a change of size, before the frame paints, never glided. A move up
 * lets go, and Latest, over the composer, goes back. A change of language keeps the reader's place. */
export function Thread({
  question,
  who,
  children,
  resting = true,
  busy = false,
  sent = null,
  hold = false,
}: {
  question: string;
  who: string;
  children: ReactNode;
  /** The thread has something to rest on: land at its end, unless the lawyer has moved away. */
  resting?: boolean;
  /** The lawyer's latest question sent from here, by a key of its own: each new one goes to the end and follows. */
  sent?: string | null;
  /** A jump to one of the thread's entries is landing (Find): nothing follows it away. */
  hold?: boolean;
  /** Rows are going in all at once, such as the first view or an older page: the log is marked aria-busy, so they
   * aren't read out as they land. */
  busy?: boolean;
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
  // Latest shows and hides on its element too, as the pin does: following runs on every streamed line
  const [follow] = useState(() =>
    createFollow(threadPlace(scope.nodes), (show) => showLatest(scope, show)),
  );

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

  // where the view rests: the thread's end, while following; once there is something to rest on, and again once the
  // fonts settle
  useLayoutEffect(() => {
    if (!resting) return;
    const rest = () => {
      if (thread.current === null || inner.current === null) return;
      follow.grew();
      check();
      readAnchor();
    };
    rest();
    let on = true;
    void document.fonts.ready.then(() => {
      if (on) rest();
    });
    return () => {
      on = false;
    };
  }, [resting, follow]);

  // following: back to the end whenever the thread or what it holds changes size, which a ResizeObserver reports
  // after layout and before paint, so no frame shows the words below the fold. The thread's own size changes as the
  // composer grows or the window does
  useEffect(() => {
    const t = thread.current;
    const i0 = inner.current;
    if (t === null || i0 === null) return;
    const ro = new ResizeObserver(() => follow.grew());
    ro.observe(i0);
    ro.observe(t);
    return () => ro.disconnect();
  }, [follow]);

  // a question sent from here goes to the end, and follows its answer
  useLayoutEffect(() => {
    if (sent !== null) follow.end(false);
  }, [sent, follow]);

  // Find's jump: nothing follows while it lands
  useLayoutEffect(() => follow.hold(hold), [hold, follow]);

  // a change of language or size keeps the reader's place
  const first = useRef(true);
  useLayoutEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    // at the end the view stays there; elsewhere it keeps the line being read
    if (follow.following) follow.grew();
    else keepAnchor();
    check();
  }, [lang, textSize, follow]);

  useEffect(() => {
    const t = thread.current;
    if (t === null) return;
    let raf = 0;
    const onScroll = () => {
      if (raf !== 0) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        follow.scrolled();
        check();
        readAnchor();
      });
    };
    // the lawyer's own moves up let go at once. On a wide window only those over the thread: the rail and the
    // papers scroll on their own. Under 1000 px the page is the thread's scroller
    const over = (e: Event) => !isWide() || (e.target instanceof Node && t.contains(e.target));
    const onWheel = (e: WheelEvent) => {
      if (e.deltaY < 0 && over(e)) follow.leave();
    };
    const onTouch = (e: TouchEvent) => {
      if (over(e)) follow.leave();
    };
    const onKey = (e: KeyboardEvent) => {
      const up = UP.has(e.key) || (e.key === " " && e.shiftKey);
      const mine = e.target instanceof Node && (t.contains(e.target) || e.target === document.body);
      if (up && mine) follow.leave();
    };
    // a press on the thread's scrollbar, past its content's width
    const onPress = (e: PointerEvent) => {
      if (e.target === t && e.offsetX >= t.clientWidth) follow.leave();
    };
    // a question's Show all or Show less (question-fold.tsx), by pointer or key: the lawyer is reading the question,
    // so the view lets go of the end before the question grows or shrinks, and stays where it is. In the capture
    // phase, before React's own listener at the root changes the question
    const onFold = (e: MouseEvent) => {
      if (e.target instanceof Element && e.target.closest("[data-fold]") !== null) follow.leave();
    };
    t.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    window.addEventListener("wheel", onWheel, { passive: true });
    window.addEventListener("touchmove", onTouch, { passive: true });
    window.addEventListener("keydown", onKey);
    t.addEventListener("pointerdown", onPress);
    t.addEventListener("click", onFold, true);
    return () => {
      cancelAnimationFrame(raf);
      t.removeEventListener("scroll", onScroll);
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      window.removeEventListener("wheel", onWheel);
      window.removeEventListener("touchmove", onTouch);
      window.removeEventListener("keydown", onKey);
      t.removeEventListener("pointerdown", onPress);
      t.removeEventListener("click", onFold, true);
    };
  }, [follow]);

  // Latest: to the end, gliding where motion is allowed, and following again. It hides as it goes, so a focus on it
  // moves on to the composer's field rather than falling to the page
  const toLatest = () => {
    const had = reach.current.nodes.get("latest")?.contains(document.activeElement) === true;
    follow.end(true);
    if (had) reach.current.nodes.get("field")?.focus({ preventScroll: true });
  };

  // the pinned question pressed: the thread glides back to the question (at once under reduced motion or in lite) and
  // the focus goes to it, so the keyboard and a screen reader go on from there. A folded question stays folded
  const backToQuestion = () => {
    const q = reach.current.nodes.get("questionStep");
    const t = thread.current;
    if (q === null || t === null) return;
    follow.leave();
    const behavior = motionAllowed() ? "smooth" : "instant";
    if (!isWide()) window.scrollBy({ top: q.getBoundingClientRect().top - 72, behavior });
    else t.scrollTo({ top: Math.max(0, q.offsetTop - 16), behavior });
    q.focus({ preventScroll: true });
    check();
  };

  const pinWords = startOf(question, PIN_CHARS);
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
          <span className="room-pin-text" lang={langOfText(pinWords)}>
            <Say>{pinWords}</Say>
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
        aria-busy={busy || undefined}
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
      {/* outside the log, so its showing is never read out as news; it stands over the thread's foot, just above the
          composer (thread.css) */}
      <div className="room-latest" ref={scope.nodes.slot("latest")} hidden>
        <Button
          kind="tone"
          size="small"
          aria-label={th(w.lang, "Go to the latest message")}
          onClick={toLatest}
        >
          <IconArrowDown />
          {th(w.lang, "Latest")}
        </Button>
      </div>
    </>
  );
}
