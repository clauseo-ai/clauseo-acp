// The Hindi of words-tools.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-tools.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
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
  "Download failed": null,
  "Try again": null,
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsWords: Hindi<keyof typeof HI> = HI;
