// The new matter form's checks, pure: the next date as the lawyer types it, read into a calendar date, and the line
// the form shows when the host refuses a folder or a matter can't be made. Never the host's own message.
import type { Lang } from "../lib/display.ts";
import { isolate } from "../lib/bidi.ts";
import { mt } from "../lib/words-matters.ts";
import { ERROR_WORDS, errorWords } from "../lib/words-errors.ts";

const EN_MONTHS = [
  "january",
  "february",
  "march",
  "april",
  "may",
  "june",
  "july",
  "august",
  "september",
  "october",
  "november",
  "december",
];

/** Hindi month names as the form's placeholder writes them, "30 अक्टूबर 2026", with the common spellings beside. */
const HI_MONTHS: readonly (readonly string[])[] = [
  ["जनवरी"],
  ["फ़रवरी", "फरवरी"],
  ["मार्च"],
  ["अप्रैल", "अप्रेल"],
  ["मई"],
  ["जून"],
  ["जुलाई"],
  ["अगस्त"],
  ["सितंबर", "सितम्बर"],
  ["अक्टूबर", "अक्तूबर"],
  ["नवंबर", "नवम्बर"],
  ["दिसंबर", "दिसम्बर"],
];

/** The month's index from its name: English in full or its first three letters, any case, or Hindi; -1 if none. */
function monthOf(word: string): number {
  const w = word.toLowerCase().replace(/\.$/, "");
  const en = EN_MONTHS.findIndex((m) => m === w || (w.length >= 3 && m.startsWith(w)));
  if (en >= 0) return en;
  return HI_MONTHS.findIndex((names) => names.includes(word));
}

const pad = (n: number) => String(n).padStart(2, "0");

/** A calendar date as the host keeps it, "2026-10-30", if the day exists. */
function iso(year: number, month: number, day: number): string | null {
  if (year < 1000 || year > 9999 || month < 0 || month > 11 || day < 1) return null;
  const last = new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
  return day > last ? null : `${year}-${pad(month + 1)}-${pad(day)}`;
}

/** The next date as the lawyer types it, read: "30 October 2026" (or "30 Oct 2026", or "30 अक्टूबर 2026") and
 * "2026-10-30" give "2026-10-30"; anything else, a day that doesn't exist included, gives null. Empty text is no
 * date, and gives null too: the caller tells the two apart by the text. */
export function parseNextDate(text: string): string | null {
  const s = text.trim().replace(/\s+/g, " ");
  const isoForm = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (isoForm !== null) return iso(Number(isoForm[1]), Number(isoForm[2]) - 1, Number(isoForm[3]));
  const written = /^(\d{1,2}) ([^\s\d]+),? (\d{4})$/u.exec(s);
  if (written === null) return null;
  return iso(Number(written[3]), monthOf(written[2] ?? ""), Number(written[1]));
}

/** The line under the next date when its text doesn't read as a date; null when it's empty or reads. */
export function nextDateError(lang: Lang, text: string): string | null {
  if (text.trim() === "" || parseNextDate(text) !== null) return null;
  return mt(lang, "Write the date as 30 October 2026.");
}

/** The form's line for a matter the host wouldn't make, from the error's code and, where the caller has it, the
 * folder's display ("Documents/Acme", "C:\\"), isolated in the line, and for matter.folder_is_matter the name of the
 * matter that has the folder, when the matters list has it. matter.folder_refused, which doesn't say why, takes a line
 * true of every reason the host gives it: a folder already a matter, an unreadable matter file, the matters' own
 * folder, a drive, the home folder, a system folder. H5's codes each say their reason; any other matter.folder_ code
 * words-errors has a line for gives that line, in the language; every other code gives the generic line. */
export function createErrorLine(
  lang: Lang,
  code: string,
  shown?: string,
  matterName?: string,
): string {
  const both = (first: string | null, second: string) =>
    first === null ? second : `${first} ${second}`;
  const named = shown !== undefined && shown !== "";
  const path = named ? isolate(shown) : "";
  switch (code) {
    case "matter.folder_refused":
      return mt(lang, "Clauseo can't make a matter of this folder. Choose another folder.");
    case "matter.folder_is_matter":
      return matterName === undefined || matterName === ""
        ? mt(lang, "This folder is already a matter.")
        : mt(lang, "This folder is already a matter: {name}.", { name: isolate(matterName) });
    case "matter.folder_in_matters_root":
      return both(
        named
          ? mt(lang, "{path} is the folder Clauseo keeps matters in.", { path })
          : mt(lang, "That folder is where Clauseo keeps matters."),
        mt(lang, "Choose the folder for one matter."),
      );
    case "matter.folder_is_drive":
      return both(
        named ? mt(lang, "{path} is a whole drive.", { path }) : null,
        mt(lang, "A matter is one folder of papers. Choose the folder for one matter."),
      );
    case "matter.folder_is_home":
      return both(
        named ? mt(lang, "{path} is your home folder.", { path }) : null,
        mt(lang, "It holds far more than one matter. Choose the folder for one matter."),
      );
    case "matter.folder_is_system":
      return both(
        named ? mt(lang, "{path} is a folder the computer keeps for itself.", { path }) : null,
        mt(lang, "Choose the folder that holds the matter's papers."),
      );
    case "matter.folder_unreadable":
      return both(
        named
          ? mt(lang, "Clauseo can't read {path}.", { path })
          : mt(lang, "Clauseo can't read that folder."),
        mt(lang, "It may be locked, or on a drive that's no longer connected."),
      );
    default:
      // a folder refusal with its own line in words-errors says the host's reason; anything else gives the generic line
      return code.startsWith("matter.folder_") && Object.hasOwn(ERROR_WORDS, code)
        ? errorWords(lang, code)
        : mt(lang, "Clauseo couldn't create the matter. Try again, or choose another folder.");
  }
}
