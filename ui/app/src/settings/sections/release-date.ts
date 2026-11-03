// The release date About shows, in the reader's language: "3 November 2026", "3 नवंबर 2026", Latin digits in both. The
// host sends the day as YYYY-MM-DD (UTC); it is formatted as that UTC day, so no zone moves it a day.
import type { Lang } from "@clauseo/components/lib/display.ts";

const LOCALE = { en: "en-IN", hi: "hi-IN" } as const satisfies Record<Lang, string>;
const formats = new Map<Lang, Intl.DateTimeFormat>();

const ISO_DAY = /^(\d{4})-(\d{2})-(\d{2})$/;

/** The release date in the language, long form, or null where there is no date (`date` null) or it isn't a real
 * YYYY-MM-DD day (a malformed string, February 30th). */
export function releaseDateWords(date: string | null, lang: Lang): string | null {
  const m = date === null ? null : ISO_DAY.exec(date);
  if (m === null || m[1] === undefined || m[2] === undefined || m[3] === undefined) return null;
  const year = Number(m[1]);
  const month = Number(m[2]);
  const day = Number(m[3]);
  const at = new Date(Date.UTC(year, month - 1, day));
  // Date.UTC rolls a day that isn't there into the next month, and reads years 0 to 99 as 1900 and on
  if (at.getUTCFullYear() !== year || at.getUTCMonth() !== month - 1 || at.getUTCDate() !== day) {
    return null;
  }
  let format = formats.get(lang);
  if (format === undefined) {
    format = new Intl.DateTimeFormat(LOCALE[lang], {
      day: "numeric",
      month: "long",
      year: "numeric",
      timeZone: "UTC",
      numberingSystem: "latn",
    });
    formats.set(lang, format);
  }
  return format.format(at);
}
