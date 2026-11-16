// The unsafe-site list's words (0053): the notice in the conversation where a site was blocked, its
// Allow confirm, and Settings' "Allowed sites" under Privacy. As in words.ts the English is the
// key, the exact string on screen. Both windows import this file: the room's thread chunk and
// Settings.
import type { Lang } from "./display.ts";
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-blocklist.hi.ts";

/** Every string of the unsafe-site list's parts, in English. */
export type BlocklistKey = keyof typeof HI;

/** An unsafe-site list string in the language, its slots filled. */
export const bl = wordsOf("blocklist");

/** A day as the unsafe-site list's parts say it, "8 November 2026", Latin digits in both
 * languages. */
export const blocklistDay = (at: number, lang: Lang): string =>
  new Intl.DateTimeFormat(lang === "hi" ? "hi-IN" : "en-IN", {
    day: "numeric",
    month: "long",
    year: "numeric",
    numberingSystem: "latn",
  }).format(at);

/** Why the list names a site, by its tag; nothing for a tag from a newer Clauseo. */
export function reasonKey(tag: string): BlocklistKey | null {
  if (tag === "malware") return "This site is linked to harmful software.";
  if (tag === "phishing") return "This site may try to steal passwords or details.";
  return null;
}
