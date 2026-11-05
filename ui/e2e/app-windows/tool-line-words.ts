// The tool line's words tool-line.spec.ts looks for, as the window says them (ui/components/src/lib/words-tools-line.ts
// and words-tools.ts, slots filled). tool-line-words.test.ts checks each against the words files, so a reworded line
// fails here, not on CI.

/** The tool line's accessible name begins with this while web search is on: "{tool}: {state}." with "Web search" and
 * "on". A browser sentence may follow it. */
export const WEB_ON = "Web search: on.";

/** The chip of a tool that is on, and of one that is off. */
export const ON = "On";
export const OFF = "Off";

/** Every reason the browser row can give when off, whatever the machine's browser state. */
export const BROWSER_REASONS = [
  "In Settings",
  "Managed by firm",
  "Not available yet",
  "Too many browsers open",
  "Needs Chrome",
];
