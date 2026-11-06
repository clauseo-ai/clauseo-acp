// The Hindi of words-tool-result.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-tool-result.ts imports this file's type alone.

export const HI = {
  // U17: the row's press, closed and open
  "Show the result": null, // hi: later
  "Hide the result": null, // hi: later
  // the blocks it didn't draw: pictures, then anything else
  "1 picture isn't shown here.": null, // hi: later
  "{n} pictures aren't shown here.": null, // hi: later
  "1 more item isn't shown here.": null, // hi: later
  "{n} more items aren't shown here.": null, // hi: later
  // past 12 lines, and past the shown cap
  "Show all": null, // hi: later
  "The rest of this result isn't shown here.": null, // hi: later
  "Nothing came back.": null, // hi: later
  // a blob's read past 400 ms, and a blob too large to read or whose read failed
  "Reading the result.": null, // hi: later
  "This result is too large to show here.": null, // hi: later
} as const;
