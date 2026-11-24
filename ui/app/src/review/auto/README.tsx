// A new copy-review entries file goes in review/auto/ and default-exports Entry[]; never edit registry.tsx for it.
// registry-auto.ts collects every .tsx here (tests aside) in path order and registry.tsx spreads them at the end of
// its list. Name the file after its brief (registry-uc-<brief>.tsx); a helper it needs goes beside it as a .ts file.
// This file registers nothing: it keeps the folder, and lets registry-auto.test.ts see the glob find a file.
import type { Entry } from "../entry.ts";

const NONE: Entry[] = [];
export default NONE;
