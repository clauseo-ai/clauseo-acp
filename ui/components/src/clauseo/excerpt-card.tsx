import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type RefObject,
} from "react";
import { Popover } from "@base-ui/react/popover";
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { IconOpen } from "../icons/icons.tsx";
import { motionAllowed } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { BenchMark } from "./citations.tsx";
import { useHandState, type Hand, type Target } from "./hand.ts";
import { useWide } from "./layout.ts";
import { benchOf, benchWords, keepIds, pages, paperIn, roleOf, type MatterData } from "./matter.ts";
import { useMatter } from "./matter-context.ts";
import { dur, ease } from "./motion.ts";
import { useRoom, type RoomNodes } from "./room-scope.ts";
import { Say } from "./say.tsx";

/** A paper's card: its tile's face as the band (numeral, pin, title, role), then the verbatim excerpt with the
 * phrase the answer relies on marked, "Verbatim from the paper" and Open. */
function PaperCard({ id, openRef }: { id: string; openRef: RefObject<HTMLButtonElement | null> }) {
  const w = useWords();
  const goTo = useGoTo();
  const p = useMatter().paper(id);
  if (p === undefined) return null;
  const role = roleOf(p) ?? "relied";
  const pb = pages(p.pb);
  const excerpt = p.excerpt ?? "";
  const mark = p.mark ?? "";
  const at = mark === "" ? -1 : excerpt.indexOf(mark);
  const title = p.title;
  const pin = p.pin ?? p.pb;
  return (
    <>
      <div className="card-band">
        <span className="card-num">{pb.kind === "pages" ? pb.from : pb.word}</span>
        <div className="card-head">
          <span className="card-pin">
            <Say>{keepIds(pin)}</Say>
          </span>
          <Popover.Title render={<p />} className="card-title">
            <Say>{keepIds(title)}</Say>
          </Popover.Title>
        </div>
        <Chip role={role} ground={role === "relied" ? "surface" : "tile"} className="chip-slot" />
      </div>
      <div className="card-body">
        {/* the excerpt is the paper's own words, from an English paper, so it stays English in both languages */}
        <Popover.Description render={<blockquote />} lang="en" className="card-quote as-en">
          {at < 0 ? (
            keepIds(excerpt)
          ) : (
            <>
              {keepIds(excerpt.slice(0, at))}
              <mark>{keepIds(mark)}</mark>
              {keepIds(excerpt.slice(at + mark.length))}
            </>
          )}
        </Popover.Description>
        <div className="card-foot">
          <span className="card-where">{w.t("Verbatim from the paper")}</span>
          <Button
            ref={openRef}
            kind="tone"
            size="small"
            aria-label={w.t("Open {title} at {pin}", { title, pin })}
            onClick={() => goTo({ kind: "paper", id })}
          >
            {w.t("Open")}
            <IconOpen size={14} />
          </Button>
        </div>
      </div>
    </>
  );
}

/** A judgment's card: the row's face as the band (bench, name, stance), then its court, bench and date, Good law with
 * the check's note on the well tone, and Open. */
function CiteCard({
  id,
  openRef,
  onOpenJudgment,
}: {
  id: string;
  openRef: RefObject<HTMLButtonElement | null>;
  onOpenJudgment?: OpenJudgment | undefined;
}) {
  const w = useWords();
  const goTo = useGoTo();
  const { hand } = useRoom();
  const c = useMatter().cite(id);
  if (c === undefined) return null;
  const bench = benchOf(c.court, c.bench);
  const stance = c.stance === "against" ? "against" : "for";
  const pin = [c.cite, c.pin].filter((s) => s !== null).join(", ");
  return (
    <>
      <div className="card-band">
        <BenchMark bench={bench} stance={stance} />
        <div className="card-head">
          <span className="card-pin">
            <Say>{keepIds(pin)}</Say>
          </span>
          <Popover.Title render={<p />} lang="en" className="card-title as-en">
            {c.name}
          </Popover.Title>
        </div>
        <Chip role={stance === "against" ? "cite" : "for"} className="chip-slot" />
      </div>
      <div className="card-body">
        <p className="card-bench">
          <span className="whitespace-nowrap">
            {w.t("{court}, {bench}", { court: c.court, bench: benchWords(w.lang, bench) })}
          </span>
          <span className="card-decided">{w.t("Decided {date}", { date: c.decided })}</span>
        </p>
        <Popover.Description render={<div />} className="card-status">
          <Chip role="good" icon="tick">
            {c.check.result}
          </Chip>
          <span className="font-clauseo">
            <Say>{keepIds(c.check.note)}</Say>
          </span>
        </Popover.Description>
        <div className="card-foot">
          <span className="card-where">
            {w.t("Read {waitingFor}", { waitingFor: c.check.waitingFor })}
          </span>
          <Button
            ref={openRef}
            kind="tone"
            size="small"
            aria-label={w.t("Open {name}", { name: c.name })}
            onClick={(e) => {
              if (onOpenJudgment === undefined) {
                goTo({ kind: "cite", id });
                return;
              }
              // the judgment reader grows out of the card it was opened from, so the card's place is read first, and
              // the reader is told before the card goes down, while the words that opened it are still in hand
              const from = e.currentTarget.closest(".card")?.getBoundingClientRect() ?? null;
              onOpenJudgment(id, from);
              hand.put();
            }}
          >
            {w.t("Open")}
            <IconOpen size={14} />
          </Button>
        </div>
      </div>
    </>
  );
}

/** Opens a judgment in the reader, from where its card stood. Without it, a judgment's Open goes to its row. */
export type OpenJudgment = (id: string, from: DOMRect | null) => void;

/** To a tile or a citation row in the matter, with the focus on it. */
function useGoTo() {
  const { hand, nodes } = useRoom();
  return (t: Target) => {
    hand.put();
    const el = nodes.target(t);
    if (el === null) return;
    el.scrollIntoView({ block: "nearest" });
    nodes.find(t.kind === "cite" ? "link" : "face", t.id)?.focus({ preventScroll: true });
  };
}

type Place = { key: string; x: number; y: number; w: number; tile: DOMRect };

/** On the wall the card grows out of the tile and takes the wall's whole width; its top meets the tile's row. It
 * never crosses the work or the window's edge. */
function placeOnWall(nodes: RoomNodes, target: Target, el: HTMLElement): Omit<Place, "key"> {
  const t = el.getBoundingClientRect();
  const isCite = target.kind === "cite";
  const wall = (isCite ? el : (nodes.get("facade") ?? el)).getBoundingClientRect();
  const w = Math.round(wall.width + (isCite ? 8 : 0));
  const box = nodes.windowBox();
  let x = isCite ? wall.left - 4 : wall.left;
  if (x + w > box.right - 8) x = box.right - 8 - w;
  return { x: Math.round(x), y: Math.round(isCite ? t.top - 7 : t.top), w, tile: t };
}

const clampY = (nodes: RoomNodes, y: number, h: number) => {
  const box = nodes.windowBox();
  return Math.round(Math.max(box.top + 8, Math.min(y, box.bottom - 12 - h)));
};

const shown = (h: Hand) => (h.state === "none" ? null : h);

/** The card's colour: a judgment's stays on the surface; a paper takes its role's. */
function cardRole(matter: MatterData, t: Target): string {
  if (t.kind === "cite") return "cite";
  const p = paperIn(matter, t.id);
  return (p === undefined ? null : roleOf(p)) ?? "relied";
}

/** What stands behind a reference, on request: the paper's verbatim paragraph, or the judgment's citation and check.
 * On the wide window it grows out of its tile or row across the wall; on a narrow one it sits against what was
 * tapped. Pointing opens it after 90 ms and leaving closes it after 120 ms; a click, a tap or Enter puts it in hand.
 * Escape returns the focus to the words; Tab steps on to what follows them. */
export function ExcerptCard({ onOpenJudgment }: { onOpenJudgment?: OpenJudgment } = {}) {
  const { hand: store, nodes } = useRoom();
  const hand = useHandState(store);
  const now = shown(hand);
  // the last thing shown stays in the card while it closes; then the card leaves the page
  const [last, setLast] = useState(now);
  const [leaving, setLeaving] = useState(false);
  if (now !== null && now !== last) setLast(now);
  if (now !== null && leaving) setLeaving(false);
  if (now === null && last !== null && !leaving) setLeaving(true);
  const closeTimer = useRef(0);

  // back into the tile it came from, faster than it opened: its words go first, then the shape; elsewhere it fades.
  // A timer, never the animation's promise, takes it off the page.
  useLayoutEffect(() => {
    if (!leaving) return;
    const node = nodes.get("card");
    let ms = 0;
    if (node !== null && motionAllowed()) {
      const clip = node.dataset["clip"];
      ms = dur(clip === undefined ? "leave" : "shrink");
      node.style.pointerEvents = "none";
      node.animate(
        clip === undefined
          ? [
              { opacity: 1, transform: "none" },
              { opacity: 0, transform: "scale(0.97)" },
            ]
          : [{ clipPath: "inset(0px 0px 0px 0px round 14px)" }, { clipPath: clip }],
        { duration: ms, easing: ease("leave"), fill: "forwards" },
      );
      if (clip !== undefined) {
        for (const part of node.children)
          part.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 60, fill: "forwards" });
      }
    }
    closeTimer.current = window.setTimeout(() => {
      setLast(null);
      setLeaving(false);
    }, ms);
    return () => window.clearTimeout(closeTimer.current);
  }, [leaving, nodes]);

  const h = now ?? last;
  if (h === null) return null;
  return <CardPopover h={h} now={now} onOpenJudgment={onOpenJudgment} />;
}

type Shown = Exclude<Hand, { state: "none" }>;

/** The card itself, mounted while it shows and while it leaves. */
function CardPopover({
  h,
  now,
  onOpenJudgment,
}: {
  h: Shown;
  now: Shown | null;
  onOpenJudgment?: OpenJudgment | undefined;
}) {
  const wide = useWide();
  const { hand, nodes } = useRoom();
  const matter = useMatter();
  const popup = useRef<HTMLDivElement>(null);
  const openRef = useRef<HTMLButtonElement>(null);
  const place = useRef<Place | null>(null);
  const key = `${h.target.kind}:${h.target.id}`;
  const targetNode = nodes.target(h.target);
  const onWall = wide && targetNode !== null && targetNode.getClientRects().length > 0;

  // the virtual anchor: on the wall, the card's own top-left corner, kept inside the window as its height changes
  const anchor = {
    getBoundingClientRect: (): DOMRect => {
      const p = place.current;
      const height = popup.current?.offsetHeight ?? 0;
      if (onWall && p !== null) return new DOMRect(p.x, clampY(nodes, p.y, height), p.w, 0);
      const trig = h.trigger;
      return trig.getClientRects()[0] ?? trig.getBoundingClientRect();
    },
  };

  // measure where it stands before it opens, and grow it out of its tile; run when the card mounts and whenever
  // what it shows changes
  const fit = (node: HTMLDivElement | null) => {
    if (now === null) return;
    const el = nodes.target(h.target);
    if (!onWall || el === null) {
      place.current = null;
      if (node !== null) {
        node.style.removeProperty("width");
        delete node.dataset["clip"];
      }
      return;
    }
    const fresh = place.current?.key !== key;
    if (fresh) place.current = { key, ...placeOnWall(nodes, h.target, el) };
    const p = place.current;
    if (node === null || p === null) return;
    node.style.width = `${p.w}px`;
    const height = node.offsetHeight;
    const y = clampY(nodes, p.y, height);
    const t = p.tile;
    const px = (v: number) => `${Math.max(0, Math.round(v))}px`;
    const clip = `inset(${px(t.top - y)} ${px(p.x + p.w - t.right)} ${px(y + height - t.bottom)} ${px(t.left - p.x)} round 10px)`;
    // the tile's own rectangle, for the card to shrink back into when it closes
    node.dataset["clip"] = clip;
    if (!fresh || !motionAllowed()) return;
    node.animate([{ clipPath: clip }, { clipPath: "inset(0px 0px 0px 0px round 14px)" }], {
      duration: dur("grow"),
      easing: ease("spring"),
    });
    // the card's words come in once the shape is about 60% grown, so the tile's and the card's never show at once
    for (const part of node.children) {
      part.animate([{ opacity: 0 }, { opacity: 1 }], {
        duration: dur("words-out") + 30,
        delay: dur("words-out") - 10,
        easing: "linear",
        fill: "backwards",
      });
    }
  };
  // the card mounts a commit after it opens, inside its portal: a stable ref callback sizes it as it arrives
  const fitNow = useRef(fit);
  useLayoutEffect(() => {
    fitNow.current = fit;
    fit(popup.current);
  });
  const [attach] = useState(() => (node: HTMLDivElement | null) => {
    popup.current = node;
    fitNow.current(node);
    if (node === null) return;
    // the room finds its card here, to send it back into its tile as it closes
    const off = nodes.slot("card")(node);
    return () => {
      popup.current = null;
      fitNow.current(null);
      if (typeof off === "function") off();
    };
  });

  // the words it opened from take the reference's tint and point to the excerpt
  useEffect(() => {
    if (now === null) return;
    const trig = now.trigger;
    trig.setAttribute("data-open", "");
    return () => trig.removeAttribute("data-open");
  }, [now]);

  // from the keyboard, the focus goes on into the card, to Open
  useEffect(() => {
    if (now?.state === "pinned" && now.keyboard) openRef.current?.focus({ preventScroll: true });
  }, [now]);

  // a scroll puts it down
  useEffect(() => {
    if (now === null) return;
    const down = (e: Event) => {
      const t = e.target;
      if (t instanceof Node && popup.current?.contains(t)) return;
      hand.put();
    };
    const resize = () => hand.put();
    document.addEventListener("scroll", down, { capture: true, passive: true });
    window.addEventListener("resize", resize);
    return () => {
      document.removeEventListener("scroll", down, { capture: true });
      window.removeEventListener("resize", resize);
    };
  }, [now, hand]);

  const onOpenChange = (open: boolean, details: Popover.Root.ChangeEventDetails) => {
    if (open) return;
    const ev = details.event;
    const target = ev?.target;
    // a press on something with a card is that thing's own press: it puts this card down or swaps it
    if (details.reason === "outside-press" && nodes.isTrigger(target ?? null)) {
      details.cancel();
      return;
    }
    if (details.reason === "focus-out" && now?.state === "pinned") {
      details.cancel();
      return;
    }
    const trig = now?.trigger;
    const inside = popup.current?.contains(document.activeElement) ?? false;
    hand.put();
    if (details.reason === "escape-key" && trig !== undefined && inside) hand.returnFocus(trig);
  };

  // Tab steps out of the card to what follows the words; Shift+Tab back to the words. Forward, the focus goes back
  // to the words and the Tab goes on from there, so the browser itself finds what follows them
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "Tab" || now === null) return;
    hand.put();
    if (e.shiftKey) {
      e.preventDefault();
      hand.returnFocus(now.trigger);
      return;
    }
    hand.returnFocus(now.trigger);
  };
  const onPointerLeave = (e: PointerEvent<HTMLDivElement>) => {
    if (e.pointerType !== "touch") hand.leave();
  };

  const role = cardRole(matter, h.target);

  return (
    <Popover.Root open onOpenChange={onOpenChange} modal={false}>
      <Popover.Portal>
        <Popover.Positioner
          anchor={anchor}
          side="bottom"
          align="start"
          sideOffset={onWall ? 0 : 8}
          collisionPadding={8}
          collisionAvoidance={
            onWall ? { side: "none", align: "none" } : { side: "flip", align: "shift" }
          }
          positionMethod="fixed"
          className="z-30"
        >
          <Popover.Popup
            ref={attach}
            className="card"
            data-role={role}
            data-from={onWall ? "wall" : "anchor"}
            initialFocus={false}
            finalFocus={false}
            onPointerEnter={hand.stay}
            onPointerLeave={onPointerLeave}
            onKeyDown={onKeyDown}
          >
            {h.target.kind === "cite" ? (
              <CiteCard id={h.target.id} openRef={openRef} onOpenJudgment={onOpenJudgment} />
            ) : (
              <PaperCard id={h.target.id} openRef={openRef} />
            )}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
