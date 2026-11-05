// The Hindi of listOf (words-core.ts), keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-core.ts imports this file's type alone.
export const HI = {
  // the last two of a list, as a sentence joins them (listOf puts commas between the rest)
  "{a} and {b}": "{a} और {b}",
} as const;
