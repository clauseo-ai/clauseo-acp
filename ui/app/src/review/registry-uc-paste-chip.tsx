// The long paste's copy review entries (paste-chips.tsx): the composer with a pasted text of about eight pages or more
// as its chip, "Pasted text" over its size in words (UC-256, UC-257, or UC-259 for a text of one word), with Remove
// pasted text (UC-258), shown with its button focused. The text is the probe's fixture (long-question-fixtures.ts).
// The composer starts with a draft holding it as a chip.
import { PASTE_CHIP_MIN } from "@clauseo/components/clauseo/paste-chips-logic.ts";
import { longQuestion } from "../room/long-question-fixtures.ts";
import type { Entry } from "./entry.ts";
import { Pasted } from "./paste-chip-state.tsx";
import { threadWords } from "./words.ts";

const BRIEF = longQuestion(4_000);
/** One word of PASTE_CHIP_MIN characters, such as a pasted run of digits. */
const ONE_WORD = "7".repeat(PASTE_CHIP_MIN);

/** The paste chip's entries, each id written as it is. */
export const UC_PASTE_CHIP: Entry[] = [
  {
    id: "UC-256",
    words: threadWords("Pasted text"),
    screen: "room/composer",
    render: () => <Pasted text={BRIEF} />,
  },
  {
    id: "UC-257",
    words: threadWords("{count} words"),
    screen: "room/composer",
    render: () => <Pasted text={BRIEF} />,
  },
  {
    id: "UC-258",
    words: threadWords("Remove pasted text"),
    screen: "room/composer",
    render: () => <Pasted text={BRIEF} focus />,
  },
  {
    id: "UC-259",
    words: threadWords("1 word"),
    screen: "room/composer",
    render: () => <Pasted text={ONE_WORD} />,
  },
];
