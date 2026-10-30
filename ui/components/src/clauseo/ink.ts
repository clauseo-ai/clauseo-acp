// Accepting without reflow. The lawyer's weight is heavier, and a heavier cut sets wider, so the
// weight can't move alone or the lines would rewrap as it settles. Each step of the weight is paired with the width
// that keeps the words' measure, found by measuring this answer's own words in this browser's own Anek, per block and
// per script. The lines are held exactly where they stood while accepted, and the answer keeps its height.
import { settleCurve } from "./logic.ts";
import { dur, ease } from "./motion.ts";

type Curve = { w: number; s: number }[];
const curves = new Map<string, Curve>();

/** For one block: the width paired with each weight, so its words never set wider than at Clauseo's weight. */
export function curve(node: HTMLElement): Curve {
  const cs = getComputedStyle(node);
  const base = Number.parseFloat(cs.fontStretch) || 100;
  const text = (node.textContent ?? "").slice(0, 900);
  const key = `${base}|${cs.fontSize}|${cs.letterSpacing}|${cs.fontFamily}|${text.length}|${node.lang || document.documentElement.lang}`;
  const known = curves.get(key);
  if (known !== undefined) return known;
  const probe = document.createElement("span");
  probe.textContent = text;
  probe.setAttribute("aria-hidden", "true");
  const st = probe.style;
  st.position = "absolute";
  // measured and removed in the same task, so it never paints
  st.left = "0";
  st.top = "0";
  st.visibility = "hidden";
  st.whiteSpace = "nowrap";
  st.fontFamily = cs.fontFamily;
  st.fontSize = cs.fontSize;
  st.letterSpacing = cs.letterSpacing;
  st.fontKerning = cs.fontKerning;
  st.fontFeatureSettings = cs.fontFeatureSettings;
  node.append(probe);
  const out = settleCurve(base, (w, s) => {
    st.fontWeight = String(w);
    st.fontStretch = `${s}%`;
    return probe.getBoundingClientRect().width;
  });
  probe.remove();
  curves.set(key, out);
  return out;
}

/** Where each line of a block starts: the text nodes and offsets at which the text moves down a line. */
function breaks(node: HTMLElement): { tn: Text; i: number }[] {
  const out: { tn: Text; i: number }[] = [];
  const range = document.createRange();
  const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
  let lastTop = Number.NEGATIVE_INFINITY;
  for (let n = walker.nextNode(); n !== null; n = walker.nextNode()) {
    const tn = n as Text;
    const d = tn.data;
    for (let i = 0; i < d.length; i++) {
      // a line can only start at the beginning of a run, after a space or after a hyphen
      if (i > 0 && !/[\s\u00A0-]/.test(d[i - 1] ?? "")) continue;
      if (/\s/.test(d[i] ?? "")) continue;
      range.setStart(tn, i);
      range.setEnd(tn, i + 1);
      const r = range.getClientRects()[0];
      if (r === undefined) continue;
      if (r.top > lastTop + 2) {
        if (lastTop > Number.NEGATIVE_INFINITY) out.push({ tn, i });
        lastTop = r.top;
      }
    }
  }
  return out;
}

/** The breaks held in each answer, to take out again. */
const held = new WeakMap<HTMLElement, HTMLBRElement[]>();

/** Pins each line break where it stands, with a real break, so accepting can't rewrap a line. blocks: the answer's
 * blocks that take the lawyer's weight. */
export function hold(answer: HTMLElement, blocks: readonly HTMLElement[]): void {
  release(answer);
  const brs: HTMLBRElement[] = [];
  held.set(answer, brs);
  for (const node of blocks) {
    const bs = breaks(node);
    for (let k = bs.length - 1; k >= 0; k--) {
      const b = bs[k];
      if (b === undefined) continue;
      const br = document.createElement("br");
      br.className = "held-break";
      brs.push(br);
      const parent = b.tn.parentNode;
      if (parent === null) continue;
      if (b.i === 0) parent.insertBefore(br, b.tn);
      else parent.insertBefore(br, b.tn.splitText(b.i));
    }
  }
}

/** Takes the held breaks out again; the text flows on its own. */
export function release(answer: HTMLElement): void {
  const brs = held.get(answer);
  if (brs === undefined) return;
  held.delete(answer);
  for (const br of brs) br.remove();
}

/** Each block's own width at 600, from its curve; the stylesheet falls back to 101.5% without it. */
export function setWidths(blocks: readonly HTMLElement[]): void {
  for (const n of blocks) {
    n.style.removeProperty("--ink-s");
    const c = curve(n);
    const end = c[c.length - 1];
    if (end !== undefined) n.style.setProperty("--ink-s", `${end.s}%`);
  }
}

/** The settle itself: weight and width move together along the curve, 280 ms on the arrive curve. */
export function settle(blocks: readonly HTMLElement[], accepted: boolean): Animation[] {
  const out: Animation[] = [];
  for (const n of blocks) {
    const frames = curve(n).map((p) => ({ fontWeight: String(p.w), fontStretch: `${p.s}%` }));
    if (!accepted) frames.reverse();
    out.push(n.animate(frames, { duration: dur("accept"), easing: ease("arrive") }));
  }
  return out;
}
