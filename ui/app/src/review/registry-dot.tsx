// The matter refusals whose fix is a folder whose name starts with a dot (words-errors-dot.ts): a matter folder holding
// .claude/scheduled_tasks.json (matter.claude_tasks_file) or a .git folder or file (matter.git_folder). Each id is
// U3-err-<code> with the agent's variant (.chatgpt, .named, .nameless) and, on the Mac, .mac: Claude's own line off
// the Mac is U3-err-<code> itself (registry.tsx's ERRORS). Each renders the turn's stop line, on the computer it names,
// for the thread's agent; the nameless lines render alone, as no thread the room opens lacks a name. Provisional.
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { errorWords as sentence } from "@clauseo/components/lib/words-errors.ts";
import type { ByAgent } from "@clauseo/components/lib/words-errors-backslash.ts";
import {
  GIT_FOLDER,
  GIT_FOLDER_MAC,
  TASKS_FILE,
  TASKS_FILE_MAC,
} from "@clauseo/components/lib/words-errors-dot.ts";
import { step, userMessage } from "../testing/stream-script.ts";
import type { AgentId, ErrorCode } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { errorWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "What does clause nine of the lease say about notice?");

/** A turn `agent` couldn't start, on `platform`, refused with `code`. */
const stopped = (code: ErrorCode, agent: AgentId, platform: Platform) => () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [QUESTION, step(9, { stop: "failed", failure: code })],
        agent,
        platform,
      })
    }
  />
);

/** The nameless line, alone, as the room would say it. */
const alone = (code: ErrorCode, platform: Platform) => (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>{sentence(lang, code, { kind: undefined, name: null }, { platform, parent: null })}</p>
  </main>
);

type Key = Parameters<typeof errorWords>[0];

/** Each code's lines off the Mac and on it. */
const CODES: readonly { code: ErrorCode; off: ByAgent; mac: ByAgent }[] = [
  { code: "matter.claude_tasks_file", off: TASKS_FILE, mac: TASKS_FILE_MAC },
  { code: "matter.git_folder", off: GIT_FOLDER, mac: GIT_FOLDER_MAC },
];

export const DOT_ENTRIES: Entry[] = CODES.flatMap(({ code, off, mac }) =>
  (
    [
      [".mac", mac.claude, stopped(code, "claude-acp", "mac")],
      [".chatgpt", off.chatgpt, stopped(code, "codex-acp", "windows")],
      [".chatgpt.mac", mac.chatgpt, stopped(code, "codex-acp", "mac")],
      [".named", off.named, stopped(code, "gemini", "windows")],
      [".named.mac", mac.named, stopped(code, "gemini", "mac")],
      [".nameless", off.nameless, alone(code, "windows")],
      [".nameless.mac", mac.nameless, alone(code, "mac")],
    ] as const
  ).map(([suffix, key, render]) => ({
    id: `U3-err-${code}${suffix}`,
    words: errorWords(key as Key),
    screen: "room/thread",
    render,
  })),
);
