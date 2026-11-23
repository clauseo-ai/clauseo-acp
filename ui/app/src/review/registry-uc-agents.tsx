// UC's entries for words Clauseo gives the agent rather than the window: the line every ChatGPT (Codex) session is told
// about a command or a save the sandbox or Clauseo refuses, so the lawyer reads in the answer what never shows as a
// step; the sentence both agents are told about a paper the lawyer mentions with "@"; and their clean-edit sentence
// (no tracked changes when the lawyer asks). Rust sends them
// (crates/profile/assets/instructions/codex-refusal.txt and tools.txt), and Rust tests there (tests/legal/subagents.rs
// and tests/legal/rules.rs) fail unless the lines below are those files' words, verbatim. The mock shows each as the
// agent reads it.
import type { Entry } from "./entry.ts";
import type { WordsRef } from "./words.ts";

/** A line of the agent's instructions, in English alone: the agent reads it, and answers in the lawyer's language. */
const instructionWords = (file: string, en: string): WordsRef => ({
  file: `crates/profile/assets/instructions/${file}`,
  key: en,
  text: () => en,
  later: true,
});

const REFUSAL = instructionWords(
  "codex-refusal.txt",
  "When Clauseo or Codex's reviewer refuses a command or a save, tell the lawyer plainly in your answer what you could not do and why, for example \"I couldn't save the copy there: Clauseo keeps my work inside this matter's folders.\" Never leave a refusal out of your answer, and never try another way round it.",
);

const MENTION = instructionWords(
  "tools.txt",
  "When the lawyer mentions a paper with @, read it before you answer; for a Word document, use stetdoc.",
);

const CLEAN_EDIT = instructionWords(
  "tools.txt",
  "When the lawyer asks for no tracked changes, a clean edit or no markup, use `stetdoc edit --no-track` and say it is not tracked, so Word has nothing to accept or reject, and the earlier version is in Versions. Use `--manual` only when the lawyer asks for manual markup by name.",
);

/** The line as the agent's instructions hold it, on a card of its own. */
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
  {
    id: "UC-711",
    words: MENTION,
    screen: "agent/instructions",
    width: 520,
    height: 200,
    render: instructions(MENTION),
  },
  {
    id: "UC-985",
    words: CLEAN_EDIT,
    screen: "agent/instructions",
    width: 520,
    height: 220,
    render: instructions(CLEAN_EDIT),
  },
];
