// The pen. While Clauseo writes, an endless line of longhand writes itself in the working line, so the lawyer can see
// the answer is still coming. Ported from clauseo-v2's longhand mark.
//
// The loop has no seam. The stroke is one unbroken ligature: words part by shallow glides along the baseline, never
// by a pen lift, so the reveal has no gap to stutter across. The path is exactly periodic: every unit advances
// exactly UNIT_DX and starts and ends on the baseline moving level, so winding the clock back one period draws the
// same frame. The ink shows through its dash while the paper drifts left, smoothed, so the nib keeps its place and
// bobs with the stroke. Each frame writes three SVG attributes in one requestAnimationFrame; React never re-renders.
//
// Cost: the frame runs only while the pen is writing, the page is shown and the pen is in view, at most 30 times a
// second (the nib moves under a pixel a frame at that rate, so it looks the same as 60). Under reduced motion and in
// lite it draws one still frame of writing and never asks for a frame. The ink is hidden until the first frame is
// set, so a first paint never shows the whole scribble.
import { useEffect, useRef } from "react";
import { useDisplay } from "../lib/display.ts";
import { inkLookup } from "./ink-lookup.ts";

const N = (v: number) => +v.toFixed(2);

/** One letter's arch: up h and back down over w. */
const hump = (h: number, w: number) =>
  `c ${N(w * 0.25)} 0 ${N(w * 0.35)} ${N(-h)} ${N(w * 0.5)} ${N(-h)} ` +
  `c ${N(w * 0.15)} 0 ${N(w * 0.25)} ${N(h)} ${N(w * 0.5)} ${N(h)}`;
/** An ascender with a curl at its top (dx exactly 6.8). */
const ASC = "c 1 -5 1.5 -11 3.5 -13 c 1.6 -1.6 3 -0.6 2.4 1.4 c -0.9 3.2 -0.2 8.2 0.9 11.6";
/** A descender's loop under the baseline (dx exactly 4). */
const DESC = "c 2 5 1 8 -1 8 c -1.8 0 -2 -2.5 -0.6 -4 c 1.2 -1.2 3.4 -2.2 5.6 -4";
/** Between words the pen slides along the baseline, never lifting. */
const glide = (w: number) =>
  `c ${N(w * 0.3)} 0 ${N(w * 0.5)} -2.2 ${N(w * 0.7)} -2.2 ` +
  `c ${N(w * 0.12)} 0 ${N(w * 0.22)} 2.2 ${N(w * 0.3)} 2.2`;

/** Three words and their glides, advancing exactly UNIT_DX. */
const UNIT = [
  hump(7, 10),
  hump(7.5, 9),
  ASC,
  hump(6.5, 10),
  glide(13),
  hump(8, 10.5),
  DESC,
  hump(7, 9.5),
  glide(12),
  hump(6.5, 9),
  hump(7, 10),
  ASC,
  hump(7.5, 10),
  hump(6, 9),
  glide(14.4),
].join(" ");
const UNITS = 8;
const UNIT_DX = 144;
/** Where, in design units, the paper's smoothing holds the nib before the view's shift. */
const PEN_REF = 250;
/** The window's width in design units: 110 by 36, so 61 px wide at the row's 20 px line. */
const VIEW_W = 110;
/** The nib's place across the window. */
const PEN_AT = 0.78;
/** Writing speed in path units a second. */
const SPEED = 36;
/** The fewest milliseconds between two drawn frames: 30 a second. */
const FRAME_MS = 1000 / 30;
const PATH_D = `M 0 0 ${Array(UNITS).fill(UNIT).join(" ")}`;

/** The pen writing. `writing` false holds its last frame still and stops asking for frames: the turn is done. */
export function PenMark({ writing = true, className }: { writing?: boolean; className?: string }) {
  const d = useDisplay();
  const still = d.motion !== "full" || d.render !== "full";
  const svgRef = useRef<SVGSVGElement>(null);
  const panRef = useRef<SVGGElement>(null);
  const inkRef = useRef<SVGPathElement>(null);
  const nibRef = useRef<SVGGElement>(null);
  // where the pen is along the path and where the paper stands, kept across a pause so writing resumes in place
  const clock = useRef<{ penK: number; paperX: number } | null>(null);

  useEffect(() => {
    const svg = svgRef.current;
    const pan = panRef.current;
    const ink = inkRef.current;
    const nib = nibRef.current;
    if (!svg || !pan || !ink || !nib) return;
    const LEN = ink.getTotalLength();
    const PU = LEN / UNITS;
    const shift = PEN_REF - VIEW_W * PEN_AT;
    // the clock stays in [4.35, 5.35] PU, so the table covers it with margin
    const at = inkLookup((k) => ink.getPointAtLength(k), PU * 3.3, PU * 5.4);
    const c = (clock.current ??= {
      penK: PU * 4.35,
      paperX: ink.getPointAtLength(PU * 4.35).x - PEN_REF,
    });
    const draw = () => {
      const p = at(c.penK);
      pan.setAttribute("transform", `translate(${N(-c.paperX - shift)} 0)`);
      ink.setAttribute("stroke-dasharray", `${N(c.penK)} ${N(LEN)}`);
      nib.setAttribute("transform", `translate(${N(p.x)} ${N(p.y)})`);
      nib.setAttribute("opacity", "1");
      return p;
    };
    draw();
    if (still || !writing) return;

    let raf = 0;
    let last = 0;
    let shown = !document.hidden;
    let inView = true;
    const frame = (now: number) => {
      raf = requestAnimationFrame(frame);
      if (now - last < FRAME_MS - 2) return;
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      // a slight wobble in speed, so the hand isn't metronomic
      c.penK += SPEED * (1 + 0.15 * Math.sin(c.penK * 0.05)) * dt;
      // back one exact period: the same frame, by construction
      if (c.penK > PU * 5.35) {
        c.penK -= PU;
        c.paperX -= UNIT_DX;
      }
      const p = draw();
      c.paperX += (p.x - PEN_REF - c.paperX) * (1 - Math.exp(-dt * 6));
    };
    const sync = () => {
      const go = shown && inView;
      if (go && raf === 0) {
        last = performance.now();
        raf = requestAnimationFrame(frame);
      } else if (!go && raf !== 0) {
        cancelAnimationFrame(raf);
        raf = 0;
      }
    };
    const onVisibility = () => {
      shown = !document.hidden;
      sync();
    };
    document.addEventListener("visibilitychange", onVisibility);
    const io =
      typeof IntersectionObserver === "function"
        ? new IntersectionObserver((entries) => {
            const e = entries[entries.length - 1];
            if (e === undefined) return;
            inView = e.isIntersecting;
            sync();
          })
        : null;
    io?.observe(svg);
    sync();
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      io?.disconnect();
      cancelAnimationFrame(raf);
      raf = 0;
    };
  }, [still, writing]);

  return (
    <svg
      ref={svgRef}
      className={className === undefined ? "pen-mark" : `pen-mark ${className}`}
      viewBox={`0 -24 ${VIEW_W} 36`}
      preserveAspectRatio="xMidYMid meet"
      aria-hidden="true"
      focusable="false"
      data-still={still || !writing ? "" : undefined}
    >
      <g ref={panRef}>
        <path
          ref={inkRef}
          d={PATH_D}
          fill="none"
          stroke="currentColor"
          strokeWidth={1.75}
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeDasharray="0 99999"
        />
        <g ref={nibRef} opacity={0}>
          <path d="M0 0 L4.6 -8.6" stroke="currentColor" strokeWidth={1.15} strokeLinecap="round" />
          <circle r={1.05} fill="currentColor" />
        </g>
      </g>
    </svg>
  );
}
