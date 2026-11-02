// U6f's copy review entries (ids U6F-nn: the review's ids take capitals only, status.ts): the agent.* sentences that
// name Claude, said for the open conversation's plan (words-errors.ts's plannedKey). Each is the real refusal on a
// send, as U3-err-agent.* shows Claude's, in a thread whose agent is Codex (the ChatGPT plan) or an agent of no kind (a
// settings file's own id).
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
    words: errorWords("Clauseo couldn't start this conversation. Your question is kept."),
    screen: "room/composer",
    render: refused("agent.launch_failed", "my-agent"),
  },
  {
    id: "U6F-03",
    words: errorWords("ChatGPT sent something Clauseo can't read."),
    screen: "room/composer",
    render: refused("agent.protocol_error", "codex-acp"),
  },
  {
    id: "U6F-04",
    words: errorWords("Clauseo got a reply it can't read."),
    screen: "room/composer",
    render: refused("agent.protocol_error", "my-agent"),
  },
  {
    id: "U6F-05",
    words: errorWords("Sign in to ChatGPT in Codex first. Your question is kept."),
    screen: "room/composer",
    render: refused("agent.auth_required", "codex-acp"),
  },
  {
    id: "U6F-06",
    words: errorWords("This conversation needs a sign-in first. Your question is kept."),
    screen: "room/composer",
    render: refused("agent.auth_required", "my-agent"),
  },
  {
    id: "U6F-07",
    words: errorWords("This version of Codex doesn't work with Clauseo."),
    screen: "room/composer",
    render: refused("agent.unsupported_version", "codex-acp"),
  },
  {
    id: "U6F-08",
    words: errorWords(
      "Clauseo doesn't work with the program this conversation uses, in this version.",
    ),
    screen: "room/composer",
    render: refused("agent.unsupported_version", "my-agent"),
  },
];
