// U6f's copy review entries (ids U6F-nn: the review's ids take capitals only, status.ts): agent.launch_failed's sentence for the open conversation's plan (words-errors.ts's
// launchFailedKey). Each is the real refusal on a send, as U3-err-agent.launch_failed shows Claude's, in a thread whose
// agent is Codex (the ChatGPT plan) or an agent of no kind (a settings file's own id).
import type { AgentId } from "../transport/protocol.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomAsking } from "./states.tsx";
import { errorWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "Summarise the notice to the bank.");
const ANSWER = agentText(6, 1, "Clause nine requires **notice in writing**, served at the branch.");
const ENDED = step(7, { stop: "end_turn", ts: Date.now() - 5 * 60_000 });

/** A send refused because the thread's agent didn't start. */
const refused = (agent: AgentId) => () => (
  <RoomAsking
    host={() =>
      threadHost({ entries: [QUESTION, ANSWER, ENDED], agent, refuse: "agent.launch_failed" })
    }
    question="And clause ten?"
  />
);

/** U6f's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U6F: Entry[] = [
  {
    id: "U6F-01",
    words: errorWords("ChatGPT didn't start. Your question is kept."),
    screen: "room/composer",
    render: refused("codex-acp"),
  },
  {
    id: "U6F-02",
    words: errorWords("Clauseo couldn't start this conversation. Your question is kept."),
    screen: "room/composer",
    render: refused("my-agent"),
  },
];
