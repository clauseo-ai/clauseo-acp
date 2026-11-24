// The change note's press for each paper it names (R24), as the wall's list gives them: a paper the wall lists, of a
// type the desktop opens (open-in-word.ts's isOpenable), opens with the computer's default app through open_file, named
// for the program where the type says it: "Open in Word" for a Word document, "Open in Excel" for a spreadsheet, "Open"
// for the rest (the host doesn't name the default app). A paper of a kind no program opens from Clauseo gets Show in
// Finder (File Explorer, folder), which shows the matter's folder, as does a paper the wall's list doesn't hold yet (a
// paper the answer just made, until the list catches up). A deleted paper gets none.
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { revealKey, ui } from "@clauseo/components/lib/words-ui.ts";
import type { FileChange } from "../../transport/protocol.ts";
import { isWordFile } from "../word/lock.ts";
import { type WordOpen, type WordOpener } from "../word/open-in-word.ts";

/** A paper's press on the note: its words, its test id, and the opener it presses; reveal: it shows the folder. */
export type CardOpen = { label: string; testId: string; opener: WordOpener; reveal: boolean };

const SPREADSHEET = /\.xlsx?$/i;

/** What a reveal-only paper presses: the folder's show, with no open of its own. */
const revealer = (label: string, onReveal: () => Promise<string | null>): WordOpener => ({
  label,
  protectedLine: "",
  onReveal,
  onOpen: () => Promise.resolve({ refused: null, protectedView: false }),
});

/** Each named paper's press, by its place in papers (matter paths, as the note names them). */
export function cardOpens(
  papers: readonly string[],
  files: readonly FileChange[],
  word: WordOpen,
  lang: Lang,
  platform: Platform,
): (CardOpen | null)[] {
  return papers.map((path) => {
    if (files.find((f) => f.path === path)?.change === "deleted") return null;
    const opener = word.wallOpenerFor(path);
    if (opener !== null) {
      if (isWordFile(path))
        return { label: opener.label, testId: "open-in-word", opener, reveal: false };
      const label = SPREADSHEET.test(path) ? mt(lang, "Open in Excel") : t(lang, "Open");
      return { label, testId: "open-paper", opener, reveal: false };
    }
    // a Word document the list holds waits for the reader's words, which name its press
    if (word.reveal === null || (isWordFile(path) && word.listed(path))) return null;
    const label = ui(lang, revealKey(platform));
    return { label, testId: "show-paper", opener: revealer(label, word.reveal), reveal: true };
  });
}
