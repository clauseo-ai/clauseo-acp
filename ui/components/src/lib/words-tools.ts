// The words of the tool line's popover: each of Clauseo's tools by name, where it stands in the open conversation,
// and the Chrome download the browser row offers. As in words-settings.ts the English is the key, the exact string on
// screen, and a key with slots fills them in either language. The popover loads after the first paint, so these
// words do too; the status line's own phrase is words-tools-line.ts's. The Hindi comes later: each
// entry is null (hi: later) and shows its English.
import { useDisplay } from "./display.ts";
import { say } from "./words-ui.ts";
import { wordsOf, type Hindi, type Slots } from "./words-core.ts";

const HI = {
  // hi: later: every string here ships in English, its Hindi null until the Hindi pass
  // each tool's name
  "Web search": null,
  Browser: null,
  // where web search stands
  "On. Clauseo can search the web and read pages.": null,
  "Off. This {name} login can't search the web. Clauseo can still read web pages.": null,
  // where either tool stands when it is off: {managed} is the settings window's "Managed by your organisation"
  "Off. You turned it off in Settings.": null,
  "Off. {managed}.": null,
  "Open Settings": null,
  // under the rows when Open Settings is offered: a change in Settings never reaches the open conversation
  "Changes apply to new conversations.": null,
  // where the browser stands
  "On. Clauseo can open web pages in its own browser.": null,
  "Off. Your organisation chooses the tools {name} has on this computer, so Clauseo can't add its browser.":
    null,
  "Off until Clauseo can check each website the browser opens.": null,
  "Off. Other conversations are using every browser this computer can run. One frees up when another conversation closes.":
    null,
  "Off. There's no Chrome on this computer.": null,
  // while Chrome downloads or unpacks, the browser row's first line says what it waits for
  "Off until Chrome is downloaded.": null,
  // the Chrome download; a number and its unit are held together by a non-breaking space (\u00a0)
  "Clauseo can download its own Chrome from Google, about 200\u00a0MB, once for all conversations. It needs about 1\u00a0GB free.":
    null,
  "Download Chrome": null,
  "Install Google Chrome to use the browser.": null,
  "Downloading Chrome. {done} of {total}\u00a0MB.": null,
  "Unpacking Chrome.": null,
  "Chrome is ready. New conversations get the browser.": null,
  // a failed download, by the host's code
  "The connection dropped before Chrome finished downloading.": null,
  "The download didn't match the file Google published, so Clauseo deleted it.": null,
  "Chrome couldn't be downloaded. It needs about 1\u00a0GB free on this computer.": null,
  "Your network is blocking the download.": null,
  "Chrome downloaded but couldn't be unpacked.": null,
  "Clauseo can't download Chrome itself. Install Google Chrome to use the browser.": null,
  "The download stopped before it finished.": null,
  "Chrome couldn't be downloaded.": null,
  "Try again": null,
} as const;

/** Every string of the tool line's popover, in English. */
export type ToolsKey = keyof typeof HI;

/** A popover string in the language, its slots filled. */
export const tw = wordsOf<ToolsKey>(HI);

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsWords: Hindi<ToolsKey> = HI;

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
