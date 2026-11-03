// The words of the tool line's popover: each tool's state chip, Open Settings, and the Chrome download the browser
// row offers, in the fewest words (DESIGN.md §Icons). As in words-settings.ts the English is the key, the exact
// string on screen, and a key with slots fills them in either language. The popover loads after the first paint, so
// these words do too; the tools' names and reasons are words-tools-line.ts's. The Hindi comes later: each entry is
// null (hi: later) and shows its English.
import { useDisplay } from "./display.ts";
import { wordsOf, type Hindi, type Slots } from "./words-core.ts";

const HI = {
  // hi: later: every string here ships in English, its Hindi null until the Hindi pass
  // each row's state chip
  On: null,
  Off: null,
  // the popover's foot when a tool is off in Settings, and its tooltip: a change never reaches the open conversation
  "Open Settings": null,
  "Changes apply to new conversations.": null,
  // the Chrome download; a number and its unit are held together by a non-breaking space (\u00a0)
  "Download Chrome": null,
  "200\u00a0MB": null,
  "From Google, once. Needs 1\u00a0GB free.": null,
  "Install Chrome to use it": null,
  "{done} of {total}\u00a0MB": null,
  Unpacking: null,
  // Chrome is in: the browser row's reason, and its tooltip on when the browser comes
  "Ready for new conversations": null,
  "The browser comes with your next conversation.": null,
  // a failed download, by the host's code
  "Connection dropped": null,
  "File didn't match": null,
  "Needs 1\u00a0GB free": null,
  "Network blocked it": null,
  "Couldn't unpack": null,
  "Download stopped": null,
  "Download failed": null,
  "Try again": null,
} as const;

/** Every string of the tool line's popover, in English. */
export type ToolsKey = keyof typeof HI;

/** A popover string in the language, its slots filled. */
export const tw = wordsOf<ToolsKey>(HI);

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsWords: Hindi<ToolsKey> = HI;

/** The popover's words in the page's language, bound: const w = useToolsWords(); w.t("Download Chrome"). */
export function useToolsWords() {
  const { lang } = useDisplay();
  return { lang, t: (key: ToolsKey, slots?: Slots) => tw(lang, key, slots) };
}
