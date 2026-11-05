// U6f's copy review entries (ids U6F-nn: the review's ids take capitals only, status.ts): the agent.* sentences that
// name Claude, said for the open conversation's agent (words-errors.ts's plannedKey). Each is the real refusal on a
// send, as U3-err-agent.* shows Claude's, in a thread whose agent is Codex (the ChatGPT plan) or an agent of no kind,
// named by its settings id. The nameless lines (U6F-09 to U6F-12) show only where no name is known at all, which the
// room never has today (a thread always names its agent's id), so they render by themselves.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorWords as sentence } from "@clauseo/components/lib/words-errors.ts";
import type { AgentId, ErrorCode } from "../transport/protocol.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomAsking } from "./states.tsx";
import { errorWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "Summarise the notice to the bank.");
const ANSWER = agentText(6, 1, "Clause nine requires **notice in writing**, served at the branch.");
const ENDED = step(7, { stop: "end_turn", ts: Date.now() - 5 * 60_000 });

/** A send refused with `code` in a thread run by `agent`. */
const refused = (code: ErrorCode, agent: AgentId) => () => (
  <RoomAsking
    host={() => threadHost({ entries: [QUESTION, ANSWER, ENDED], agent, refuse: code })}
    question="And clause ten?"
  />
);

/** A nameless line, alone: no thread the room opens lacks a name, so it is shown as the sentence the room would say. */
const nameless = (code: ErrorCode) => (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>{sentence(lang, code, { kind: undefined, name: null })}</p>
  </main>
);

/** U6f's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U6F: Entry[] = [
  {
    id: "U6F-01",
    words: errorWords("ChatGPT didn't start. Your question is kept."),
    screen: "room/composer",
    render: refused("agent.launch_failed", "codex-acp"),
  },
  {
    id: "U6F-02",
    words: errorWords("{name} didn't start. Your question is kept."),
    screen: "room/composer",
    render: refused("agent.launch_failed", "gemini"),
  },
  {
    id: "U6F-03",
    words: errorWords("ChatGPT sent something Clauseo can't read."),
    screen: "room/composer",
    render: refused("agent.protocol_error", "codex-acp"),
  },
  {
    id: "U6F-04",
    words: errorWords("{name} sent something Clauseo can't read."),
    screen: "room/composer",
    render: refused("agent.protocol_error", "gemini"),
  },
  {
    id: "U6F-05",
    words: errorWords("Sign in to ChatGPT in Codex first. Your question is kept."),
    screen: "room/composer",
    render: refused("agent.auth_required", "codex-acp"),
  },
  {
    id: "U6F-06",
    words: errorWords("Sign in to {name} first. Your question is kept."),
    screen: "room/composer",
    render: refused("agent.auth_required", "gemini"),
  },
  {
    id: "U6F-07",
    words: errorWords("This version of Codex doesn't work with Clauseo."),
    screen: "room/composer",
    render: refused("agent.unsupported_version", "codex-acp"),
  },
  {
    id: "U6F-08",
    words: errorWords("This version of {name} doesn't work with Clauseo."),
    screen: "room/composer",
    render: refused("agent.unsupported_version", "gemini"),
  },
  {
    id: "U6F-09",
    words: errorWords("The AI didn't start. Your question is kept."),
    screen: "room/composer",
    render: nameless("agent.launch_failed"),
  },
  {
    id: "U6F-10",
    words: errorWords("The AI sent something Clauseo can't read."),
    screen: "room/composer",
    render: nameless("agent.protocol_error"),
  },
  {
    id: "U6F-11",
    words: errorWords("Sign in to the AI first. Your question is kept."),
    screen: "room/composer",
    render: nameless("agent.auth_required"),
  },
  {
    id: "U6F-12",
    words: errorWords("This version of the AI doesn't work with Clauseo."),
    screen: "room/composer",
    render: nameless("agent.unsupported_version"),
  },
  {
    id: "U6F-13",
    words: errorWords("Clauseo can't start ChatGPT safely here. Reinstall Clauseo."),
    screen: "room/composer",
    render: refused("agent.guard_missing", "codex-acp"),
  },
  {
    id: "U6F-14",
    words: errorWords("Clauseo can't start {name} safely here. Reinstall Clauseo."),
    screen: "room/composer",
    render: refused("agent.guard_missing", "gemini"),
  },
  {
    id: "U6F-15",
    words: errorWords("Clauseo can't start the AI safely here. Reinstall Clauseo."),
    screen: "room/composer",
    render: nameless("agent.guard_missing"),
  },
];
