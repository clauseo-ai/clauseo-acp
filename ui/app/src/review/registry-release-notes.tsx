// The release notes' "Opening Clauseo the first time" block (UC-751 to UC-755), which GitHub shows on the release
// page of an unsigned beta, not the app: release.yml's assemble job writes it as Markdown, only while the beta is
// unsigned, and the Windows line only while SignPath is off. Its words live in .github/workflows/release.yml, and a
// test there (.github/scripts/latest-json.test.mjs) fails unless the block's lines, read as GitHub shows them (bold,
// links and code as plain text), are the strings below in this order, so what the reviewers approve is what the
// release page says. English only: the release page has no Hindi. The state is a mock of the block on the page.
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { ReactNode } from "react";
import type { Entry } from "./entry.ts";
import type { WordsRef } from "./words.ts";

/** One line of the block, as GitHub shows it; English only (hi: later). */
const notesWords = (en: string): WordsRef => ({
  file: ".github/workflows/release.yml",
  key: en,
  text: () => en,
  later: true,
});

const HEADING = notesWords("Opening Clauseo the first time");
const WHY = notesWords(
  "Clauseo is a small open-source project with no funding yet, so this beta isn't signed by Apple or Microsoft. Your computer asks before it opens Clauseo the first time.",
);
const MAC = notesWords(
  "Mac: drag Clauseo onto Applications and open it. macOS says it can't be opened: click Done or OK, not the button that moves it to the Bin. Then open System Settings, click Privacy & Security, scroll down to Security and click Open Anyway. On macOS 15 and later, right-clicking the app and choosing Open no longer gets past this block. Installing Clauseo on a Mac shows each click.",
);
const DAMAGED = notesWords(
  'If macOS says Clauseo "is damaged": this one line in Terminal removes the "downloaded from the internet" mark from Clauseo alone: xattr -dr com.apple.quarantine /Applications/Clauseo.app. Then open Clauseo again. On a work Mac, ask your IT team before you do this.',
);
const WINDOWS = notesWords(
  "Windows: a blue box says Windows protected your PC. Click More info, then Run anyway. Installing Clauseo on Windows shows each click.",
);

/** The block as the release page shows it, in either language's cell (the page is English). */
const block = (_lang: Lang): ReactNode => (
  <div className="review-notes-ground">
    <article className="review-notes">
      <h2>{HEADING.text("en")}</h2>
      <p>{WHY.text("en")}</p>
      <ul>
        <li>{MAC.text("en")}</li>
        <li>{DAMAGED.text("en")}</li>
        <li>{WINDOWS.text("en")}</li>
      </ul>
    </article>
  </div>
);

const notes = (id: string, words: WordsRef): Entry => ({
  id,
  words,
  screen: "release/first-open",
  width: 900,
  height: 640,
  render: block,
});

/** The block's entries, each id written as it is, in the block's order (the test reads them in this order). */
export const RELEASE_NOTES: Entry[] = [
  notes("UC-751", HEADING),
  notes("UC-752", WHY),
  notes("UC-753", MAC),
  notes("UC-754", DAMAGED),
  notes("UC-755", WINDOWS),
];
