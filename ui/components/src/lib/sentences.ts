// Words said one after another, as an accessible name: separate sentences in the page's language.
import type { Lang } from "./display.ts";

/** Parts said one after another as sentences: each but the last closed with the language's stop (the danda in Hindi)
 * unless it ends in one, and no empty part. */
export function sentences(lang: Lang, parts: readonly string[]): string {
  const said = parts.map((p) => p.trim()).filter((p) => p !== "");
  return said
    .map((p, i) =>
      i === said.length - 1 || /[.?!\u0964]$/.test(p) ? p : `${p}${lang === "hi" ? "\u0964" : "."}`,
    )
    .join(" ");
}
