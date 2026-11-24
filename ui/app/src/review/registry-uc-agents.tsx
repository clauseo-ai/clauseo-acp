// UC's entries for words Clauseo gives the agent rather than the window: the line every ChatGPT (Codex) session is told
// about a command or a save the sandbox or Clauseo refuses, so the lawyer reads in the answer what never shows as a
// step; the sentence both agents are told about a paper the lawyer mentions with "@"; their clean-edit sentence
// (no tracked changes when the lawyer asks); Claude's line on listing and reading files with its own tools; and
// ChatGPT's AGENTS.md on when to start sub-agents. Rust sends them (crates/profile/assets/instructions/:
// codex-refusal.txt, tools.txt, claude-files.txt, codex-agents.md), and Rust tests there (tests/legal/subagents.rs and
// tests/legal/rules.rs) fail unless the lines below are those files' words, verbatim. The mock shows each as the
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

const CLAUDE_FILES = instructionWords(
  "claude-files.txt",
  "To list, search or read files in the matter folder or your notes folder, use the Glob, Grep and Read tools. Do not use PowerShell or Bash commands for listing, searching or reading files.",
);

/** Codex's AGENTS.md: a file of its own (codex-agents.md), which the host writes to Codex's home. */
const CODEX_AGENTS = instructionWords(
  "codex-agents.md",
  // The profile crate's rules test finds this text JSON-encoded, so it keeps double quotes.
  // oxfmt-ignore
  "In this workspace the user explicitly asks for sub-agents in one case: when a task needs more than eight documents or about 300 pages read. Then, after converting the papers to text, put every text copy longer than about 15,000 characters into 4 to 6 batches of similar size and start one sub-agent per batch, all at once. Start each sub-agent fresh, without your conversation history (fork_turns=\"none\"). Give each the full paths of its text files and what the task needs from them, in a few lines. While they work, read the short papers yourself. Wait for them, treat each report as a lead, and check every quote, date and amount you rely on against the text copies. Start at most six sub-agents for one answer. In every other case do the work yourself, unless the lawyer asks for helpers in their message.",
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
  {
    id: "UC-1200",
    words: CLAUDE_FILES,
    screen: "agent/claude-instructions",
    width: 520,
    height: 200,
    render: instructions(CLAUDE_FILES),
  },
  {
    id: "UC-1201",
    words: CODEX_AGENTS,
    screen: "agent/codex-instructions",
    width: 560,
    height: 300,
    render: instructions(CODEX_AGENTS),
  },
];
