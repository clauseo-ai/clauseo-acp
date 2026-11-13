// U6f's copy review entries (ids U6F-nn: the review's ids take capitals only, status.ts): the agent.* sentences that
// name Claude, said for the open conversation's agent (words-errors.ts's plannedKey). Each is the real refusal on a
// send, as U3-err-agent.* shows Claude's, in a thread whose agent is Codex (the ChatGPT plan) or an agent of no kind,
// named by its settings id. The nameless lines (U6F-09 to U6F-12) show only where no name is known at all, which the
// room never has today (a thread always names its agent's id), so they render by themselves. U3-52 and U3-53, the
// thread's line for a turn Clauseo stopped on the agent's request for files outside the matter, name the agent the same
// way: Codex's turn as ChatGPT, and the AI alone. So do U3-54 and U3-55, the line for a turn the model refused as too
// long to read (StopReason too_long). U6F-19 to U6F-21 are agent.service_error's, and U6F-22 to U6F-24 agent.quiet's,
// never a send's refusal: the stop line under the answer so far, with Try again.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { type AgentName, errorWords as sentence } from "@clauseo/components/lib/words-errors.ts";
import type { AgentId, ErrorCode } from "../transport/protocol.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { refusedKeyRoom } from "./registry-ud2.tsx";
import { RoomAsking, RoomOver } from "./states.tsx";
import { errorWords, threadWords } from "./words.ts";

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

/** A turn `agent` gave up on with agent.service_error: the stop line under the answer so far, with Try again. */
const serviceFailed = (agent: AgentId) => () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [QUESTION, ANSWER, step(7, { stop: "failed", failure: "agent.service_error" })],
        agent,
      })
    }
  />
);

/** A turn `agent` ended with agent.quiet (the agent sent nothing for 5 minutes): the stop line, with Try again. */
const quietStopped = (agent: AgentId) => () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [QUESTION, ANSWER, step(7, { stop: "failed", failure: "agent.quiet" })],
        agent,
      })
    }
  />
);

/** A nameless line, alone: no thread the room opens lacks a name, so it is shown as the sentence the room would say. */
const nameless = (code: ErrorCode) => (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>{sentence(lang, code, { kind: undefined, name: null })}</p>
  </main>
);

/** A set-up failure's line for an agent, alone, as the install line says it (install.tsx): ChatGPT's plan, an agent
 * named Codex on the install line, or none. Provisional (install resume PR). */
const setupFailed = (agent: AgentName) => (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>{sentence(lang, "agent.install_setup_failed", agent)}</p>
  </main>
);

/** agent.auth_required for ChatGPT on an API key OpenAI refused, in the real room (UC-149): the thread's line and the
 * card under the field say the key wasn't accepted, never a sign-in, and Codex's own 401 notice shows nothing. Its
 * cf-ray and request id are made up. */
const keyRefused = refusedKeyRoom({
  notice: {
    order: 3,
    event: "00000000-0000-4000-8000-000000000301",
    level: "warning",
    text: "Falling back from WebSockets to HTTPS transport. unexpected status 401 Unauthorized: Incorrect API key provided: sk-proj-****************alid. You can find your API key at https://platform.openai.com/account/api-keys., url: wss://api.openai.com/v1/responses, cf-ray: 9f00000000000000-BOM, request id: req_00000000000000000000000000000000, auth error: 401, auth error code: invalid_api_key",
    ts: 3500,
  },
});

/** U6f's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U6F: Entry[] = [
  {
    id: "UC-149",
    words: errorWords("{name} didn't accept the API key. Your question is kept."),
    screen: "room/composer",
    render: keyRefused,
  },
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
    words: errorWords("Sign in with ChatGPT in Codex first. Your question is kept."),
    screen: "room/composer",
    render: refused("agent.auth_required", "codex-acp"),
  },
  {
    id: "U6F-06",
    words: errorWords("Sign in with {name} first. Your question is kept."),
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
    words: errorWords("Sign in first. Your question is kept."),
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
  {
    id: "U6F-16",
    words: errorWords("Codex was downloaded, but setting it up didn't finish. Try again."),
    screen: "room/first-run",
    render: setupFailed({ kind: "codex", name: "Codex" }),
  },
  {
    id: "U6F-17",
    words: errorWords("{name} was downloaded, but setting it up didn't finish. Try again."),
    screen: "room/first-run",
    render: setupFailed({ kind: undefined, name: "Codex" }),
  },
  {
    id: "U6F-18",
    words: errorWords("The AI was downloaded, but setting it up didn't finish. Try again."),
    screen: "room/first-run",
    render: setupFailed({ kind: undefined, name: null }),
  },
  {
    id: "U6F-19",
    words: errorWords("ChatGPT had a problem on its side. Try again."),
    screen: "room/thread",
    render: serviceFailed("codex-acp"),
  },
  {
    id: "U6F-20",
    words: errorWords("{name} had a problem on its side. Try again."),
    screen: "room/thread",
    render: serviceFailed("gemini"),
  },
  {
    id: "U6F-21",
    words: errorWords("The AI had a problem on its side. Try again."),
    screen: "room/thread",
    render: nameless("agent.service_error"),
  },
  {
    id: "U6F-22",
    words: errorWords("ChatGPT stopped answering. Try again."),
    screen: "room/thread",
    render: quietStopped("codex-acp"),
  },
  {
    id: "U6F-23",
    words: errorWords("{name} stopped answering. Try again."),
    screen: "room/thread",
    render: quietStopped("gemini"),
  },
  {
    id: "U6F-24",
    words: errorWords("The AI stopped answering. Try again."),
    screen: "room/thread",
    render: nameless("agent.quiet"),
  },
  {
    id: "U3-52",
    words: threadWords(
      "Clauseo stopped the answer: {name} tried to open files outside this matter. The answer so far is kept.",
    ),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={() =>
          threadHost({
            entries: [QUESTION, ANSWER, step(7, { stop: "outside_matter" })],
            agent: "codex-acp",
          })
        }
      />
    ),
  },
  {
    id: "U3-53",
    words: threadWords(
      "Clauseo stopped the answer: the AI tried to open files outside this matter. The answer so far is kept.",
    ),
    screen: "room/thread",
    render: (lang: Lang) => (
      <main lang={lang} className="review-alone">
        <p>
          {th(
            lang,
            "Clauseo stopped the answer: the AI tried to open files outside this matter. The answer so far is kept.",
          )}
        </p>
      </main>
    ),
  },
  {
    id: "U3-54",
    words: threadWords(
      "That was too long for {name} to read at once. Shorten your question, or ask it in a new conversation. Your question and the answer so far are kept.",
    ),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={() =>
          threadHost({
            entries: [QUESTION, ANSWER, step(7, { stop: "too_long" })],
            agent: "codex-acp",
          })
        }
      />
    ),
  },
  {
    id: "U3-55",
    words: threadWords(
      "That was too long for the AI to read at once. Shorten your question, or ask it in a new conversation. Your question and the answer so far are kept.",
    ),
    screen: "room/thread",
    render: (lang: Lang) => (
      <main lang={lang} className="review-alone">
        <p>
          {th(
            lang,
            "That was too long for the AI to read at once. Shorten your question, or ask it in a new conversation. Your question and the answer so far are kept.",
          )}
        </p>
      </main>
    ),
  },
];
