// U3-err-matter.git_folder's ids for the variants the send refusal reads by agent: ChatGPT's, a named agent's and the
// nameless one (words-errors.ts's GIT_FOLDER_PLANNED), and each of the four with its Mac line (GIT_FOLDER_MAC), picked on
// macOS only. Claude's base line is registry.tsx's ERRORS entry. Each shows its sentence alone, as the review's
// registry-u6f.tsx shows a nameless line. Provisional; Hindi later.
import type { Lang } from "@clauseo/components/lib/display.ts";
import {
  errorWords as sentence,
  GIT_FOLDER_MAC,
  GIT_FOLDER_PLANNED,
} from "@clauseo/components/lib/words-errors.ts";
import type { AgentName } from "@clauseo/components/lib/words-errors.ts";
import type { Entry } from "./entry.ts";
import { errorWords } from "./words.ts";

/** One variant's sentence, alone, for the agent and computer it is read for. */
const alone = (agent: AgentName, platform: "mac" | "linux") => (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>{sentence(lang, "matter.git_folder", agent, { platform })}</p>
  </main>
);

const CHATGPT: AgentName = { kind: "codex", name: null };
const NAMED: AgentName = { kind: undefined, name: "gemini" };
const NAMELESS: AgentName = { kind: undefined, name: null };

/** The entries: each variant's id, its words and its render, for the plain line and for its Mac line. */
export const GIT_FOLDER_ENTRIES: Entry[] = [
  {
    id: "U3-err-matter.git_folder.chatgpt",
    words: errorWords(GIT_FOLDER_PLANNED.chatgpt),
    screen: "room/thread",
    render: alone(CHATGPT, "linux"),
  },
  {
    id: "U3-err-matter.git_folder.named",
    words: errorWords(GIT_FOLDER_PLANNED.named),
    screen: "room/thread",
    render: alone(NAMED, "linux"),
  },
  {
    id: "U3-err-matter.git_folder.nameless",
    words: errorWords(GIT_FOLDER_PLANNED.nameless),
    screen: "room/thread",
    render: alone(NAMELESS, "linux"),
  },
  {
    id: "U3-err-matter.git_folder.mac",
    words: errorWords(GIT_FOLDER_MAC.claude),
    screen: "room/thread",
    render: alone(undefined, "mac"),
  },
  {
    id: "U3-err-matter.git_folder.chatgpt.mac",
    words: errorWords(GIT_FOLDER_MAC.chatgpt),
    screen: "room/thread",
    render: alone(CHATGPT, "mac"),
  },
  {
    id: "U3-err-matter.git_folder.named.mac",
    words: errorWords(GIT_FOLDER_MAC.named),
    screen: "room/thread",
    render: alone(NAMED, "mac"),
  },
  {
    id: "U3-err-matter.git_folder.nameless.mac",
    words: errorWords(GIT_FOLDER_MAC.nameless),
    screen: "room/thread",
    render: alone(NAMELESS, "mac"),
  },
];
