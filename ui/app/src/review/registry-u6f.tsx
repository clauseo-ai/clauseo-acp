// U6f's copy review entries (ids U6F-nn: the review's ids take capitals only, status.ts): the agent.* sentences that
// name Claude, said for the open conversation's agent (words-errors.ts's plannedKey). Each is the real refusal on a
// send, as U3-err-agent.* shows Claude's, in a thread whose agent is Codex (the ChatGPT plan) or an agent of no kind,
// named by its settings id. The nameless lines (U6F-09 to U6F-12) show only where no name is known at all, which the
// room never has today (a thread always names its agent's id), so they render by themselves. U3-52 and U3-53, the
// thread's line for a turn Clauseo stopped on the agent's request for files outside the matter, name the agent the same
// way: Codex's turn as ChatGPT, and the AI alone.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { type AgentName, errorWords as sentence } from "@clauseo/components/lib/words-errors.ts";
import type { AgentId, ErrorCode } from "../transport/protocol.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
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

/** agent.auth_required for a plan that runs on an API key, alone, as the thread and the composer say it (UC-149): the
 * key wasn't accepted, never a sign-in. */
const keyRefused = (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>{sentence(lang, "agent.auth_required", { kind: "codex", name: "codex-acp", key: true })}</p>
  </main>
);

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
];
