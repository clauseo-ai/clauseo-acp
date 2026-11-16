// matter.folder_backslash's lines and the backslash warning's words beyond the first round's (U6F-25 to U6F-27, U3's
// own entry, FR-24 to FR-26): the refusal on Linux, whose file manager has no one name, and for a backslash in a folder
// above the matter's, named, for each agent (U6F-31 to U6F-42); New matter's warning on Linux and for a folder above
// the one picked (FR-27 to FR-29); and the first screen's Open the folder, which asks the same (FR-30). Drafts, Rohan
// to approve; the Hindi is provisional. Apart from registry-u6f.tsx and registry-folder-rights.tsx for their length.
import type { ReactNode } from "react";
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { errorWords as sentence } from "@clauseo/components/lib/words-errors.ts";
import { BACKSLASH } from "@clauseo/components/lib/words-errors-backslash.ts";
import { step, userMessage } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { AgentId } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER, NEW_MATTER, threadHost, type Step } from "./hosts.ts";
import { render as scene } from "./registry-ud1-scene.tsx";
import { RoomAfter, RoomOver } from "./states.tsx";
import { errorWords, folderWords as w, type WordsRef } from "./words.ts";

/** The matter's folder as the host shows it: its own name with the backslash, or a folder above it with one. */
const OWN = "Documents/Acme\\ Traders";
const PARENT = "Documents/Old\\Work/Acme Traders";
const QUESTION = userMessage(5, 1, "What does clause nine of the lease say about notice?");

/** A turn `agent` couldn't start, on `platform`, in a matter folder that shows as `folder`. */
const stopped = (agent: AgentId, platform: Platform, folder: string) => () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [QUESTION, step(9, { stop: "failed", failure: "matter.folder_backslash" })],
        agent,
        platform,
        matterFolder: folder,
      })
    }
  />
);

/** The nameless line, alone, as the room would say it: no thread the room opens lacks a name. */
const alone = (platform: Platform, parent: string | null) => (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>
      {sentence(
        lang,
        "matter.folder_backslash",
        { kind: undefined, name: null },
        { platform, parent },
      )}
    </p>
  </main>
);

type Key = Parameters<typeof errorWords>[0];

/** The refusal's three new sets, each for Claude, ChatGPT, an agent by its name and the nameless line. */
const SETS = [
  { platform: "linux", folder: OWN, parent: null, set: BACKSLASH.files.own },
  { platform: "mac", folder: PARENT, parent: "Old\\Work", set: BACKSLASH.finder.parent },
  { platform: "linux", folder: PARENT, parent: "Old\\Work", set: BACKSLASH.files.parent },
] as const;

const REFUSALS: Entry[] = SETS.flatMap(({ platform, folder, parent, set }, i) =>
  (
    [
      [set.claude, stopped("claude-acp", platform, folder)],
      [set.chatgpt, stopped("codex-acp", platform, folder)],
      [set.named, stopped("gemini", platform, folder)],
      [set.nameless, alone(platform, parent)],
    ] as const
  ).map(([key, render], j) => ({
    id: `U6F-${31 + i * 4 + j}`,
    words: errorWords(key as Key),
    screen: "room/thread",
    render,
  })),
);

/** New matter, on `platform`, whose picker gives a folder that shows as `display`: its warning under the folder line. */
const picked = (platform: Platform, display: string) => () => (
  <RoomAfter
    host={() =>
      new FakeTransport({
        platform,
        query: (query) =>
          typeof query === "object" && "folder_support" in query
            ? { folder_support: {} }
            : { matters: [MATTER] },
        pickFolder: () => ({ folder: "review-folder", display }),
      })
    }
    steps={[...NEW_MATTER, { press: "matter-existing" }] satisfies readonly Step[]}
  />
);

/** Several lines of one id, joined with " / " as the approved page writes them. */
const joined = (parts: readonly WordsRef[]): WordsRef => ({
  file: [...new Set(parts.map((p) => p.file))].join(", "),
  key: parts.map((p) => p.key).join(" / "),
  text: (lang: Lang) => parts.map((p) => p.text(lang)).join(" / "),
  later: parts.some((p) => p.later),
});

const entry = (id: string, words: WordsRef, render: () => ReactNode): Entry => ({
  id,
  words,
  screen: "room/folder-rights",
  render,
});

const WARNINGS: Entry[] = [
  // on Linux, the folder's own name: the body names the file manager (title and name are FR-24's)
  entry(
    "FR-27",
    w(
      "Claude can't work in a folder like this; ChatGPT can. Rename it in your file manager first, or choose another folder.",
    ),
    picked("linux", "Documents/Rathi\\ Steels"),
  ),
  // a folder above the one picked has the backslash, on the Mac: its title, body and name; Locate's step says the same
  entry(
    "FR-28",
    joined([
      w('The folder "{folder}" in this folder\'s path has a backslash (\\).'),
      w(
        "Claude can't work in a folder like this; ChatGPT can. Rename that folder in Finder first, or choose another folder.",
      ),
      w("A folder in this folder's path has a backslash"),
    ]),
    picked("mac", "Documents/Old\\Work/Rathi Steels"),
  ),
  // the same on Linux: its body
  entry(
    "FR-29",
    w(
      "Claude can't work in a folder like this; ChatGPT can. Rename that folder in your file manager first, or choose another folder.",
    ),
    picked("linux", "Documents/Old\\Work/Rathi Steels"),
  ),
  // the first screen's Open the folder, a folder picked whose name has a backslash: the step (FR-24's words, Choose
  // another folder as FR-09), and Open anyway, which makes it a matter
  {
    id: "FR-30",
    words: w("Open anyway"),
    screen: "first-launch/welcome",
    render: scene({
      first: true,
      events: [],
      platform: "mac",
      clicks: ['[data-testid="open-folder"]'],
      open: {
        pickFolder: () => ({ folder: "review-folder", display: "Documents/Rathi\\ Steels" }),
      },
    }),
  },
];

export const BACKSLASH_ENTRIES: Entry[] = [...REFUSALS, ...WARNINGS];
