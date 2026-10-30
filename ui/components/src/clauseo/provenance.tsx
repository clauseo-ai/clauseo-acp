import { use, useEffect, useRef } from "react";
import { motionAllowed } from "../lib/display.ts";
import { isWide } from "./layout.ts";
import { dur } from "./motion.ts";
import { citeIn, paperIn, roleOf, type MatterData } from "./matter.ts";
import { MatterContext } from "./matter-context.ts";
import { useRoom, type RoomNodes } from "./room-scope.ts";

const SVGNS = "http://www.w3.org/2000/svg";

type Route = { sx: number; sy: number; ex: number; ey: number; d: string };

/** The climb happens in the gutter between the work and the wall: an S-curve there, then a straight run to the card. */
function path(nodes: RoomNodes, sx: number, sy: number, ex: number, ey: number): string {
  const centre = nodes.get("centre")?.getBoundingClientRect();
  const gutter = Math.round(centre?.right ?? ex) - 18;
  const gx = Math.max(sx + 24, Math.min(gutter, ex - 12));
  const reach = Math.min(ex, gx + 28);
  const s = `M${sx} ${sy} C${gx} ${sy} ${gx} ${ey} ${reach} ${ey}`;
  return reach < ex ? `${s} L${ex} ${ey}` : s;
}

/** The pinned question's foot, or the thread's top: words above it are out of view. */
function viewTop(nodes: RoomNodes, view: DOMRect): number {
  const pin = nodes.get("pin");
  return view.top + (pin === null || pin.hidden ? 0 : pin.offsetHeight);
}

/** From one reference to a card on the wall, or null when its words are out of view. From words that end their line
 * it starts at the words; from words inside a line, in the margin beside them. */
function route(nodes: RoomNodes, ref: Element, card: DOMRect): Route | null {
  const thread = nodes.get("thread");
  if (thread === null) return null;
  const view = thread.getBoundingClientRect();
  const top = viewTop(nodes, view);
  const rects = [...ref.getClientRects()].filter(
    (r) => r.width > 0 && r.bottom > top + 4 && r.top < view.bottom - 4,
  );
  if (rects.length === 0) return null;
  const near = rects.reduce((m, r) => (r.right > m.right ? r : m));
  const col = (nodes.blockOf(ref) ?? ref.parentElement ?? ref).getBoundingClientRect();
  const sy = Math.round(near.top + near.height / 2) + 0.5;
  const atEnd = near.right >= col.right - 28;
  const sx = Math.round(atEnd ? near.right + 5 : Math.max(col.right, near.right) + 12);
  const ex = Math.round(card.left);
  const ey = Math.round(Math.max(card.top + 16, Math.min(card.bottom - 16, sy))) + 0.5;
  if (ex <= sx + 4) return null;
  return { sx, sy, ex, ey, d: path(nodes, sx, sy, ex, ey) };
}

function line(g: SVGGElement, r: Route) {
  const p = document.createElementNS(SVGNS, "path");
  p.setAttribute("d", r.d);
  const start = document.createElementNS(SVGNS, "circle");
  start.setAttribute("class", "line-start");
  start.setAttribute("cx", String(r.sx));
  start.setAttribute("cy", String(r.sy));
  start.setAttribute("r", "2.5");
  const end = document.createElementNS(SVGNS, "circle");
  end.setAttribute("class", "line-end");
  end.setAttribute("cx", String(r.ex));
  end.setAttribute("cy", String(r.ey));
  end.setAttribute("r", "3");
  g.append(p, start, end);
}

/** The line's colour: the paper's role, or for a judgment primary for us and red for theirs. */
function lineRole(matter: MatterData, kind: "paper" | "cite", id: string): string {
  if (kind === "cite") return citeIn(matter, id)?.stance === "against" ? "against" : "cite";
  const p = paperIn(matter, id);
  return (p === undefined ? null : roleOf(p)) ?? "relied";
}

/** The provenance line, on request only, and the scrim under an open card. One fine 1.5 px line in the role's colour
 * joins the cited words to the card that grew from the wall: pointing at a reference draws one; pointing at a tile
 * draws one to every place the answer cites it. The Ready card and the draft's tile join the same way. Nothing draws
 * on its own, and at rest there are none. Positioned from measured boxes and redrawn on resize and scroll. */
export function Provenance({
  turned,
  draftId = null,
}: {
  turned: boolean;
  /** The prepared draft's paper id, whose tile the Ready card joins; null when there is no draft. */
  draftId?: string | null;
}) {
  const svg = useRef<SVGSVGElement>(null);
  const scrim = useRef<HTMLDivElement>(null);
  const { hand, nodes } = useRoom();
  const matter = use(MatterContext);

  useEffect(() => {
    const s = svg.current;
    const sc = scrim.current;
    if (s === null || sc === null) return;
    let tied: Element[] = [];
    let raf = 0;
    let raf2 = 0;

    const clear = () => {
      s.replaceChildren();
      for (const r of tied) r.removeAttribute("data-tied");
      tied = [];
    };

    // what the card covers on the wall goes under one scrim, snapped out to whole tiles and rows
    const cover = (card: DOMRect | null) => {
      if (card === null) {
        sc.hidden = true;
        return;
      }
      const pane = nodes.get("pane")?.getBoundingClientRect();
      if (pane === undefined) return;
      let l = card.left;
      let t = card.top;
      let r = card.right;
      let b = card.bottom;
      const covered = [
        ...nodes.all("tile"),
        ...nodes.all("cite"),
        ...nodes.all("head"),
        nodes.get("citeMore"),
      ];
      for (const n of covered) {
        if (n === null) continue;
        const q = n.getBoundingClientRect();
        if (
          q.height === 0 ||
          q.bottom <= card.top ||
          q.top >= card.bottom ||
          q.right <= card.left ||
          q.left >= card.right
        )
          continue;
        l = Math.min(l, q.left);
        t = Math.min(t, q.top);
        r = Math.max(r, q.right);
        b = Math.max(b, q.bottom);
      }
      l = Math.max(pane.left, l - 4);
      r = Math.min(pane.right, r + 4);
      t = Math.max(pane.top, t - 4);
      b = Math.min(pane.bottom, b + 4);
      sc.style.left = `${Math.round(l)}px`;
      sc.style.top = `${Math.round(t)}px`;
      sc.style.width = `${Math.round(r - l)}px`;
      sc.style.height = `${Math.round(b - t)}px`;
      if (sc.hidden) {
        sc.hidden = false;
        if (motionAllowed())
          sc.animate([{ opacity: 0 }, { opacity: 1 }], {
            duration: dur("quick"),
            easing: "linear",
          });
      }
    };

    const draw = () => {
      clear();
      const h = hand.get();
      if (!isWide()) {
        cover(null);
        return;
      }
      if (h.state !== "none") {
        const card = nodes.get("card");
        if (card === null || card.dataset["from"] !== "wall") {
          cover(null);
          return;
        }
        const cr = card.getBoundingClientRect();
        cover(cr);
        const key = `${h.target.kind}:${h.target.id}`;
        const refs =
          nodes.get("thread")?.contains(h.trigger) === true ? [h.trigger] : nodes.refsTo(key);
        const role = lineRole(matter, h.target.kind, h.target.id);
        const g = document.createElementNS(SVGNS, "g");
        g.dataset["role"] = role;
        for (const ref of refs) {
          const r = route(nodes, ref, cr);
          if (r === null) continue;
          line(g, r);
          ref.setAttribute("data-tied", role);
          tied.push(ref);
        }
        if (tied.length > 0) s.append(g);
        return;
      }
      cover(null);
      // the Ready card and the draft's tile: one line between them while either is pointed at
      const draft = hand.getDraft();
      if (turned || draftId === null || !(draft.has("card") || draft.has("tile"))) return;
      const ready = nodes.get("ready");
      const tile = nodes.find("face", draftId);
      const thread = nodes.get("thread");
      if (ready === null || tile === null || thread === null) return;
      const view = thread.getBoundingClientRect();
      const r = ready.getBoundingClientRect();
      if (r.bottom < view.top + 8 || r.top > view.bottom - 8) return;
      const t = tile.getBoundingClientRect();
      const sx = Math.round(r.right) + 4;
      const sy =
        Math.round(Math.max(view.top + 8, Math.min(view.bottom - 8, r.top + r.height / 2))) + 0.5;
      const ex = Math.round(t.left) - 2;
      const ey = Math.round(t.top + t.height / 2) + 0.5;
      const g = document.createElementNS(SVGNS, "g");
      g.dataset["role"] = "prepared";
      line(g, { sx, sy, ex, ey, d: path(nodes, sx, sy, ex, ey) });
      s.append(g);
    };

    // once a frame at most, after the card has taken its place
    const schedule = () => {
      cancelAnimationFrame(raf);
      cancelAnimationFrame(raf2);
      raf = requestAnimationFrame(() => {
        draw();
        raf2 = requestAnimationFrame(draw);
      });
    };
    const unsubscribe = hand.subscribe(schedule);
    window.addEventListener("resize", schedule);
    document.addEventListener("scroll", schedule, { capture: true, passive: true });
    schedule();
    return () => {
      unsubscribe();
      window.removeEventListener("resize", schedule);
      document.removeEventListener("scroll", schedule, { capture: true });
      cancelAnimationFrame(raf);
      cancelAnimationFrame(raf2);
      clear();
    };
  }, [turned, draftId, matter, hand, nodes]);

  return (
    <>
      <svg ref={svg} className="provenance" aria-hidden="true" focusable="false" />
      <div ref={scrim} className="wall-scrim" aria-hidden="true" hidden />
    </>
  );
}
