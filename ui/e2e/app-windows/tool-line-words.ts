// The tool line's words tool-line.spec.ts looks for, as the window shows them (ui/components/src/lib/words-tools.ts,
// slots filled). tool-line-words.test.ts checks each against the words file, so a reworded line fails here, not on CI.

/** The web search row's words while it is on. */
export const WEB_ON = "On. Clauseo can search the web and read pages.";

/** Every first line the browser row can say, whatever the machine's browser state. */
export const BROWSER_LINES = [
  "On. Clauseo can open web pages in its own browser.",
  "Off. You turned it off in Settings.",
  "Off. Managed by your organisation.",
  "Off. Your organisation chooses the tools Claude has on this computer, so Clauseo can't add its browser.",
  "Off. Your organisation chooses the tools ChatGPT has on this computer, so Clauseo can't add its browser.",
  "Off until Clauseo can check each website the browser opens.",
  "Off. Other conversations are using every browser this computer can run. One frees up when another conversation closes.",
  "Off. There's no Chrome on this computer.",
  "Off until Chrome is downloaded.",
];
