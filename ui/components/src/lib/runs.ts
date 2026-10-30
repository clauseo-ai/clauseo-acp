// Mixed runs. Inside Hindi, every English run (a file name, a product, a case name) carries
// lang="en", so shaping, hyphenation and screen readers know the script, and the face takes the Latin cut at 105% on
// the Devanagari baseline. <Mixed> (mixed.tsx) renders them; this splits them.
import type { Lang } from "./display.ts";

const DEVA = /[\u0900-\u097F]/;
const LATIN = /[A-Za-z]/;
/** An English run: from its first Latin letter, through spaces and the marks names carry, to its last letter, digit,
 * full stop or closing mark. "Lease.docx", "File Explorer", "S.P. Mani" stay whole. */
const RUN = /["'(]?[A-Za-z][A-Za-z0-9 .,'"()&/-]*[A-Za-z0-9.)"']|["'(]?[A-Za-z]/g;

export type Run = { text: string; en: boolean };

/** Text split into its English runs and the rest. In English, or with no Latin letter, it's one run, not English
 * within the page; wholly Latin text inside Hindi is one English run. */
export function runs(lang: Lang, s: string): Run[] {
  if (lang === "en" || !LATIN.test(s)) return [{ text: s, en: false }];
  if (!DEVA.test(s)) return [{ text: s, en: true }];
  const out: Run[] = [];
  let last = 0;
  for (const m of s.matchAll(RUN)) {
    const at = m.index;
    if (at > last) out.push({ text: s.slice(last, at), en: false });
    out.push({ text: m[0], en: true });
    last = at + m[0].length;
  }
  if (last < s.length) out.push({ text: s.slice(last), en: false });
  return out;
}
