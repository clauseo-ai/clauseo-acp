// Motion from the tokens. WAAPI calls read their durations and curves from tokens.css, so the
// page turn, the card's growth and the accept settle move exactly as the stylesheet's own transitions do.

const root = () => getComputedStyle(document.documentElement);

/** A duration token in milliseconds: dur("turn") reads --dur-turn. Reduced motion shortens or zeroes it there. */
export function dur(name: string): number {
  const v = root().getPropertyValue(`--dur-${name}`).trim();
  const n = Number.parseFloat(v);
  if (!Number.isFinite(n)) return 0;
  return v.endsWith("ms") ? n : v.endsWith("s") ? n * 1000 : n;
}

/** A curve token: ease("spring") reads --ease-spring, the sampled spring as linear(), or the arrive curve where the
 * browser has no linear(). */
export function ease(name: "spring" | "arrive" | "leave"): string {
  return root().getPropertyValue(`--ease-${name}`).trim() || "cubic-bezier(0.16, 1, 0.3, 1)";
}
