// The Hindi of words-scan.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-scan.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // UD2-01, UD2-02: the counts so far
  "{n} files, {mb} MB": null, // hi: later
  "1 file, {mb} MB": null, // hi: later
  // UD2-03, UD2-04: the file the walk reached, or none yet
  "Reading the folder. Now {title}.": null, // hi: later
  "Reading the folder.": null, // hi: later
  // UD2-06, UD2-07: a folder a cloud service syncs
  "This folder syncs with {service}, so changes to its papers sync too, including Clauseo's. Versions are kept on this computer, outside the folder.":
    null, // hi: later
  "Got it": null, // hi: later
  // UD2-08 to UD2-11: files kept only online
  "{n} files are kept only in {service} and download as they're read.": null, // hi: later
  "1 file is kept only in {service} and downloads as it's read.": null, // hi: later
  "{n} files are kept only online and download as they're read.": null, // hi: later
  "1 file is kept only online and downloads as it's read.": null, // hi: later
} satisfies Hindi<string>;
