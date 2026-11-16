// agent.tool_missing's lines (words-errors-tool.ts): a file Clauseo keeps for the agent isn't as Clauseo left it, so
// the agent wasn't started. Each id is U3-err-agent.tool_missing with the agent's variant (.chatgpt, .named,
// .nameless) and, on the Mac (Linux's line is the Mac's), .mac: Claude's own line on Windows is
// U3-err-agent.tool_missing itself (registry.tsx's ERRORS). Each renders the turn's stop line, on the computer it names, for the thread's agent; the nameless lines
// render alone, as no thread the room opens lacks a name. Provisional.
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { errorWords as sentence } from "@clauseo/components/lib/words-errors.ts";
import { TOOL_MISSING, TOOL_MISSING_MAC } from "@clauseo/components/lib/words-errors-tool.ts";
import { step, userMessage } from "../testing/stream-script.ts";
import type { AgentId, ErrorCode } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { errorWords } from "./words.ts";

const CODE: ErrorCode = "agent.tool_missing";
const QUESTION = userMessage(5, 1, "What does clause nine of the lease say about notice?");

/** A turn `agent` couldn't start, on `platform`. */
const stopped = (agent: AgentId, platform: Platform) => () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [QUESTION, step(9, { stop: "failed", failure: CODE })],
        agent,
        platform,
      })
    }
  />
);

/** The nameless line, alone, as the room would say it. */
const alone = (platform: Platform) => (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>{sentence(lang, CODE, { kind: undefined, name: null }, { platform, parent: null })}</p>
  </main>
);

type Key = Parameters<typeof errorWords>[0];

export const TOOL_ENTRIES: Entry[] = (
  [
    [".mac", TOOL_MISSING_MAC.claude, stopped("claude-acp", "mac")],
    [".chatgpt", TOOL_MISSING.chatgpt, stopped("codex-acp", "windows")],
    [".chatgpt.mac", TOOL_MISSING_MAC.chatgpt, stopped("codex-acp", "mac")],
    [".named", TOOL_MISSING.named, stopped("gemini", "windows")],
    [".named.mac", TOOL_MISSING_MAC.named, stopped("gemini", "mac")],
    [".nameless", TOOL_MISSING.nameless, alone("windows")],
    [".nameless.mac", TOOL_MISSING_MAC.nameless, alone("mac")],
  ] as const
).map(([suffix, key, render]) => ({
  id: `U3-err-${CODE}${suffix}`,
  words: errorWords(key as Key),
  screen: "room/thread",
  render,
}));
