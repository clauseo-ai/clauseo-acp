// agent.program_missing's and agent.program_blocked's lines (words-errors-program.ts): the agent's program is missing
// from this computer, or the computer refused to run it, so the agent didn't start; and agent.start_unreachable's
// (words-errors-start.ts), a start that couldn't reach the agent's service though the computer is online. Each id is U3-err-<code> with the
// agent's variant (.chatgpt, .named, .nameless) and, for agent.program_blocked on the Mac, .mac: Claude's own line off
// the Mac is U3-err-<code> itself (registry.tsx's ERRORS). Each renders the turn's stop line, on the computer it names,
// for the thread's agent; the nameless lines render alone, as no thread the room opens lacks a name. Provisional.
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { errorWords as sentence } from "@clauseo/components/lib/words-errors.ts";
import {
  PROGRAM_BLOCKED,
  PROGRAM_BLOCKED_MAC,
  PROGRAM_MISSING,
} from "@clauseo/components/lib/words-errors-program.ts";
import { START_UNREACHABLE } from "@clauseo/components/lib/words-errors-start.ts";
import { step, userMessage } from "../testing/stream-script.ts";
import type { AgentId, ErrorCode } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { errorWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "What does clause nine of the lease say about notice?");

/** A turn `agent` couldn't start with `code`, on `platform`. */
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
type Row = readonly [ErrorCode, string, string, Entry["render"]];

const MISSING: ErrorCode = "agent.program_missing";
const BLOCKED: ErrorCode = "agent.program_blocked";
const UNREACHABLE: ErrorCode = "agent.start_unreachable";

export const PROGRAM_ENTRIES: Entry[] = (
  [
    [MISSING, ".chatgpt", PROGRAM_MISSING.chatgpt, stopped(MISSING, "codex-acp", "windows")],
    [MISSING, ".named", PROGRAM_MISSING.named, stopped(MISSING, "gemini", "windows")],
    [MISSING, ".nameless", PROGRAM_MISSING.nameless, alone(MISSING, "windows")],
    [BLOCKED, ".mac", PROGRAM_BLOCKED_MAC.claude, stopped(BLOCKED, "claude-acp", "mac")],
    [BLOCKED, ".chatgpt", PROGRAM_BLOCKED.chatgpt, stopped(BLOCKED, "codex-acp", "windows")],
    [BLOCKED, ".chatgpt.mac", PROGRAM_BLOCKED_MAC.chatgpt, stopped(BLOCKED, "codex-acp", "mac")],
    [BLOCKED, ".named", PROGRAM_BLOCKED.named, stopped(BLOCKED, "gemini", "windows")],
    [BLOCKED, ".named.mac", PROGRAM_BLOCKED_MAC.named, stopped(BLOCKED, "gemini", "mac")],
    [BLOCKED, ".nameless", PROGRAM_BLOCKED.nameless, alone(BLOCKED, "windows")],
    [BLOCKED, ".nameless.mac", PROGRAM_BLOCKED_MAC.nameless, alone(BLOCKED, "mac")],
    [
      UNREACHABLE,
      ".chatgpt",
      START_UNREACHABLE.chatgpt,
      stopped(UNREACHABLE, "codex-acp", "windows"),
    ],
    [UNREACHABLE, ".named", START_UNREACHABLE.named, stopped(UNREACHABLE, "gemini", "windows")],
    [UNREACHABLE, ".nameless", START_UNREACHABLE.nameless, alone(UNREACHABLE, "windows")],
  ] as readonly Row[]
).map(([code, suffix, key, render]) => ({
  id: `U3-err-${code}${suffix}`,
  words: errorWords(key as Key),
  screen: "room/thread",
  render,
}));
