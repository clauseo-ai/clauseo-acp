// The words of the tool line's popover: each of Clauseo's tools by name, where it stands in the open conversation,
// and the Chrome download the browser row offers. As in words-settings.ts the English is the key, the exact string on
// screen, and a key with slots fills them in either language. The popover loads after the first paint, so these
// words do too; the status line's own phrase is words-tools-line.ts's. The Hindi waits for a translator: each value
// is the English for now.
import { useDisplay } from "./display.ts";
import { say } from "./words-ui.ts";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  // hi: later, every value in this block
  // each tool's name
  "Web search": "Web search",
  Browser: "Browser",
  // where web search stands
  "On. Clauseo can search the web and read pages.":
    "On. Clauseo can search the web and read pages.",
  "This {name} login can't search the web. Clauseo can still read web pages.":
    "This {name} login can't search the web. Clauseo can still read web pages.",
  // where either tool stands when it is off: {managed} is the settings window's "Managed by your organisation"
  "Off. You turned it off in Settings.": "Off. You turned it off in Settings.",
  "Off. {managed}.": "Off. {managed}.",
  "Open Settings": "Open Settings",
  // where the browser stands
  "On. Clauseo can open web pages in its own browser.":
    "On. Clauseo can open web pages in its own browser.",
  "Off. Your organisation chooses the tools {name} has on this computer, so Clauseo adds none.":
    "Off. Your organisation chooses the tools {name} has on this computer, so Clauseo adds none.",
  "Off until Clauseo can check each website the browser opens.":
    "Off until Clauseo can check each website the browser opens.",
  "Off. Other conversations are using every browser this computer can run. One frees up when another conversation closes.":
    "Off. Other conversations are using every browser this computer can run. One frees up when another conversation closes.",
  "Off. There's no Chrome on this computer.": "Off. There's no Chrome on this computer.",
  // the Chrome download
  "Clauseo can download Chrome for its own use: about 200 MB from Google, once for every conversation. It needs about 1 GB free.":
    "Clauseo can download Chrome for its own use: about 200 MB from Google, once for every conversation. It needs about 1 GB free.",
  "Download Chrome": "Download Chrome",
  "Install Google Chrome to use the browser.": "Install Google Chrome to use the browser.",
  "Downloading Chrome. {done} of {total} MB.": "Downloading Chrome. {done} of {total} MB.",
  "Unpacking Chrome.": "Unpacking Chrome.",
  "Chrome is ready. New conversations get the browser.":
    "Chrome is ready. New conversations get the browser.",
  "Chrome couldn't be downloaded. It needs about 1 GB free on this computer.":
    "Chrome couldn't be downloaded. It needs about 1 GB free on this computer.",
  "Your network is blocking the download.": "Your network is blocking the download.",
  "The download didn't match Google's, so Clauseo deleted it.":
    "The download didn't match Google's, so Clauseo deleted it.",
  "Chrome couldn't be downloaded.": "Chrome couldn't be downloaded.",
  "Try again": "Try again",
} as const;

/** Every string of the tool line's popover, in English. */
export type ToolsKey = keyof typeof HI;

/** A popover string in the language, its slots filled. */
export const tw = wordsOf<ToolsKey>(HI);

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsWords: Readonly<Record<ToolsKey, string>> = HI;

/** The popover's words in the page's language, bound: const w = useToolsWords(); w.t("Browser"); say renders with
 * English runs marked. */
export function useToolsWords() {
  const { lang } = useDisplay();
  return {
    lang,
    t: (key: ToolsKey, slots?: Slots) => tw(lang, key, slots),
    say,
  };
}
