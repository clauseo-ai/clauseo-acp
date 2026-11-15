// UC's entries for words Clauseo gives the agent rather than the window: the line every ChatGPT (Codex) session is told
// about a command or a save the sandbox or Clauseo refuses, so the lawyer reads in the answer what never shows as a
// step. Rust sends it (crates/profile/assets/instructions/codex-refusal.txt), and a Rust test there
// (tests/legal/subagents.rs) fails unless the line below is that file's, verbatim. The mock shows it as the agent reads it.
import type { Entry } from "./entry.ts";
import type { WordsRef } from "./words.ts";

/** A line of the agent's instructions, in English alone: the agent reads it, and answers in the lawyer's language. */
const instructionWords = (en: string): WordsRef => ({
  file: "crates/profile/assets/instructions/codex-refusal.txt",
  key: en,
  text: () => en,
  later: true,
});

const REFUSAL = instructionWords(
  "When the sandbox or Clauseo refuses a command or a save, tell the lawyer plainly in your answer what you could not do and why, for example \"I couldn't save the copy there: Clauseo keeps my work inside this matter's folders.\" Never leave a refusal out of your answer, and never try another way round it.",
);

/** The line as ChatGPT's instructions hold it, on a card of its own. */
const instructions = (words: WordsRef) => () => (
  <div className="review-confirm-ground">
    <div className="review-confirm">
      <p>{words.text("en")}</p>
    </div>
  </div>
);

/** UC's agent-facing entries, each id written as it is. */
export const UC_AGENTS: Entry[] = [
  {
    id: "UC-710",
    words: REFUSAL,
    screen: "agent/codex-instructions",
    width: 520,
    height: 260,
    render: instructions(REFUSAL),
  },
];
