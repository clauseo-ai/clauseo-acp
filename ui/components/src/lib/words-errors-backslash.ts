// matter.folder_backslash's sentences (words-errors.ts): the matter folder's path has a backslash, where Claude can't
// start, said by the computer's file manager and by which folder's name has it. Apart from words-errors.ts to keep it
// under the size limit; their Hindi is in words-errors.hi.ts with the rest.
import type { Platform } from "./display.ts";
import { isolate } from "./words-core.ts";
import type { AgentName, ErrorKey } from "./words-errors.ts";

/** A sentence for each agent: Claude's (and an agent not known), ChatGPT's, one of no kind by its `{name}`, and one with
 * no name. */
export type ByAgent = {
  claude: ErrorKey;
  chatgpt: ErrorKey;
  named: ErrorKey;
  nameless: ErrorKey;
};

/** matter.folder_backslash's sentences, by the computer's file manager (Finder on the Mac, "your file manager" on
 * Linux; Windows never refuses) and by where the backslash is: the matter folder's own name, or a folder above it,
 * named by `{folder}`. Draft English (Rohan to approve); the Hindi is provisional. */
export const BACKSLASH: Readonly<Record<"finder" | "files", Record<"own" | "parent", ByAgent>>> = {
  finder: {
    own: {
      claude:
        "Claude can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.",
      chatgpt:
        "ChatGPT can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.",
      named:
        "{name} can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.",
      nameless:
        "The AI can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.",
    },
    parent: {
      claude:
        "Claude can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in Finder, then Try again.",
      chatgpt:
        "ChatGPT can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in Finder, then Try again.",
      named:
        "{name} can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in Finder, then Try again.",
      nameless:
        "The AI can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in Finder, then Try again.",
    },
  },
  files: {
    own: {
      claude:
        "Claude can't work in this matter's folder because its name has a backslash (\\). Rename the folder in your file manager, then Try again.",
      chatgpt:
        "ChatGPT can't work in this matter's folder because its name has a backslash (\\). Rename the folder in your file manager, then Try again.",
      named:
        "{name} can't work in this matter's folder because its name has a backslash (\\). Rename the folder in your file manager, then Try again.",
      nameless:
        "The AI can't work in this matter's folder because its name has a backslash (\\). Rename the folder in your file manager, then Try again.",
    },
    parent: {
      claude:
        "Claude can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in your file manager, then Try again.",
      chatgpt:
        "ChatGPT can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in your file manager, then Try again.",
      named:
        "{name} can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in your file manager, then Try again.",
      nameless:
        "The AI can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in your file manager, then Try again.",
    },
  },
};

/** Where matter.folder_backslash's backslash is, as its line says it: the computer, for its file manager, and the name
 * of the nearest folder above the matter's whose name has the backslash, or null for the matter folder's own name
 * (or a path the UI can't see). */
export type BackslashPlace = { platform: Platform; parent: string | null };

/** The agent's sentence of the set: the thread's agent picks it as plannedKey (words-errors.ts) does. */
export function byAgent(set: ByAgent, agent: AgentName): { key: ErrorKey; name?: string } {
  if (agent === undefined || agent.kind === "claude") return { key: set.claude };
  if (agent.kind === "codex") return { key: set.chatgpt };
  const name = agent.name?.trim() ?? "";
  return name === "" ? { key: set.nameless } : { key: set.named, name };
}

/** matter.folder_backslash's sentence for the agent and the place (BACKSLASH), with its slots. */
export function backslashKey(
  agent: AgentName,
  place: BackslashPlace,
): { key: ErrorKey; slots: Record<string, string> } {
  const set = BACKSLASH[place.platform === "linux" ? "files" : "finder"];
  const { key, name } = byAgent(place.parent === null ? set.own : set.parent, agent);
  return {
    key,
    slots: {
      ...(name === undefined ? {} : { name }),
      ...(place.parent === null ? {} : { folder: isolate(place.parent) }),
    },
  };
}
