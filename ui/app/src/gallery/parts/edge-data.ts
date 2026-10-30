// The edge's invented data: the places Clauseo asks about, the matter's allowances, and what was refused. None of it
// is real.
import type { Allowance } from "@clauseo/components/clauseo/allowed.tsx";
import type { Refused } from "@clauseo/components/clauseo/edge.tsx";

/** The matter's name as filed, for the protection questions. */
export const MATTER_NAME = "Acme Ltd v. State";
/** The clock time the samples use for an answer given today. */
export const NOW = "5:12 pm";
/** The matter's own folder, as the computer writes it. */
export const MATTER_FOLDER = "Documents/Matters/Acme";

/** A folder beyond the matter, as the thread names it, and its last part, as the computer's dialog does. */
export const FOLDER = { place: "Documents/Precedents", name: "Precedents" };
/** A website the matter hasn't used before. */
export const SITE = "example.org";
/** Another website, refused. */
export const OTHER_SITE = "example.net";

export const ALLOWED: readonly Allowance[] = [
  {
    id: "precedents",
    kind: "folder",
    place: FOLDER.place,
    note: "Read, allowed on 9 March",
  },
  {
    id: "site",
    kind: "site",
    place: SITE,
    note: "Read and search, allowed at 5:12 pm today",
  },
];

export const REFUSED: readonly Allowance[] = [
  {
    id: "other-site",
    kind: "site",
    place: OTHER_SITE,
    note: "Refused on 20 March",
    refused: true,
  },
];

/** The last tries Clauseo refused, newest last. */
export const REFUSALS: readonly Refused[] = [
  { at: "5:01 pm", what: "Read Documents/Other matter" },
  { at: "5:02 pm", what: `Open ${OTHER_SITE}` },
  { at: "5:03 pm", what: "Read Documents/Old files" },
];
