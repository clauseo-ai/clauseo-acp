// U7 part 2's review states (matters/queue, queue-full, held, held-failed, steer-claude, steer-chatgpt, steer-aside):
// the real room over a host whose one thread runs, or ran, a turn with messages queued behind it or a steer sent into
// it, as H9 shapes them (transport/h9-pending.ts). The host takes the queue's commands and can refuse one with a code,
// as a message that had already gone, or a full queue. The lawyer's words are the design's demo lines (design
// words-matters.ts), in the frame's language, so a Hindi frame reads as a Hindi lawyer's room.
import type { Lang } from "@clauseo/components/lib/display.ts";
import {
  agentText,
  batch,
  event,
  header,
  reset,
  step,
  THREAD,
  TURN,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  BackgroundMark,
  HeaderEntryH9,
  ToolCallEntryH9,
  UserMessageEntryH9,
} from "../transport/h9-pending.ts";
import type {
  AgentId,
  CommandOutcome,
  ErrorCode,
  StepEntry,
  ThreadStatus,
  TurnId,
  ViewEntry,
} from "../transport/protocol.ts";
import { MATTER } from "./hosts.ts";

/** The running turn, the turn a steer sent now starts, and each queued message's own turn (from 10 on). */
const T1 = TURN;
const T2: TurnId = uuid(2, 4);
const queuedTurn = (i: number): TurnId => uuid(10 + i, 4);

/** Today at 4:mm pm on the computer's clock, as the design's room reads ("4:31 pm"). */
const pm = (minute: number) => new Date(new Date().setHours(16, minute, 0, 0)).getTime();

/** The design's demo words (03-system/src/lib/words-matters.ts), English and Hindi. */
const SAY = {
  question: ["Does the summoning order give reasons?", "क्या समन आदेश में कारण दिए गए हैं?"],
  answer: [
    "The summoning order of 14 November 2023 records no reasons. It says only that the magistrate has perused the complaint and the documents and finds sufficient ground to proceed.",
    "14 नवंबर 2023 के समन आदेश में कोई कारण दर्ज नहीं है। उसमें केवल इतना लिखा है कि दंडाधिकारी ने शिकायत और दस्तावेज़ों का अवलोकन किया है और कार्यवाही के लिए पर्याप्त आधार पाया है।",
  ],
  steerClaude: [
    "Deal with the board minutes first. The bench will ask about them.",
    "बोर्ड बैठक का कार्यवृत्त वाली बात पहले लें। पीठ उसी के बारे में पूछेगी।",
  ],
  steerAside: [
    "Check section 149(12) of the Companies Act as well.",
    "कंपनी अधिनियम की धारा 149(12) भी देखें।",
  ],
  chequeQuestion: [
    "Who signed the cheque, going by the bank mandate?",
    "बैंक मैंडेट के अनुसार चेक पर हस्ताक्षर किसने किए?",
  ],
  chequeAnswer: [
    "Accused no. 2, Mr. Rakesh Mehta. The bank mandate names him the sole signatory for the account the cheque was drawn on, so Mr. Sharma could not have signed it.",
    "अभियुक्त संख्या 2, श्री राकेश मेहता। बैंक मैंडेट में उन्हें उस खाते का एकमात्र हस्ताक्षरकर्ता बताया गया है जिससे चेक जारी हुआ, इसलिए श्री शर्मा उस पर हस्ताक्षर नहीं कर सकते थे।",
  ],
  steerChatgpt: [
    "Say too who opened the account, if the papers show it.",
    "यदि दस्तावेज़ों में हो, तो यह भी बताएँ कि खाता किसने खोला।",
  ],
  afterSteer: ["Reading the matter's papers for this.", "इसके लिए मामले के दस्तावेज़ पढ़े जा रहे हैं।"],
  boardMinutes: [
    "The board minutes of 3 March 2023 record him present when the supply contract was approved.",
    "3 मार्च 2023 की बोर्ड बैठक के कार्यवृत्त में आपूर्ति अनुबंध के अनुमोदन के समय उनकी उपस्थिति दर्ज है।",
  ],
  search: [
    "Searched Indian Kanoon for judgments that cite S.M.S. Pharmaceuticals",
    "S.M.S. Pharmaceuticals का हवाला देने वाले निर्णय Indian Kanoon पर खोजे",
  ],
} as const satisfies Record<string, readonly [string, string]>;

/** The design's ten queued messages, in the order they were queued. */
const QUEUED = [
  [
    "Also check whether the summoning order records any reasons.",
    "यह भी देखें कि समन आदेश में कोई कारण दर्ज हैं या नहीं।",
  ],
  ["Then list what we still need from the client.", "फिर बताएँ कि मुवक्किल से अभी क्या-क्या चाहिए।"],
  ["Keep the petition under 10 pages.", "याचिका 10 पृष्ठों से कम रखें।"],
  [
    "Add the date of the summoning order to the list of dates.",
    "समन आदेश की तारीख तारीखों की सूची में जोड़ें।",
  ],
  [
    "Check that Mr. Sharma signed the vakalatnama himself.",
    "देखें कि वकालतनामे पर श्री शर्मा ने स्वयं हस्ताक्षर किए हैं।",
  ],
  [
    "Note the page of the complaint that names accused no. 4.",
    "शिकायत का वह पृष्ठ नोट करें जिसमें अभियुक्त संख्या 4 का नाम है।",
  ],
  [
    "Find when Mr. Sharma resigned, if he did.",
    "पता करें कि श्री शर्मा ने कब इस्तीफ़ा दिया, यदि दिया हो।",
  ],
  ["Compare paragraph 9 of the complaint with the notice.", "शिकायत के पैरा 9 की नोटिस से तुलना करें।"],
  ["Draft a one page note for the client in Hindi.", "मुवक्किल के लिए हिंदी में एक पृष्ठ का नोट तैयार करें।"],
  ["Mark every annexure the petition cites.", "याचिका में उद्धृत हर अनुलग्नक को चिह्नित करें।"],
] as const;

const say = (lang: Lang, words: readonly [string, string]) => words[lang === "en" ? 0 : 1];

/** A user message of `turn`, at `ts`, with any of H9's fields. */
const ask = (
  order: number,
  n: number,
  text: string,
  turn: TurnId,
  ts: number,
  over: Partial<UserMessageEntryH9> = {},
): ViewEntry => ({
  user_message: { ...userMessage(order, n, text).user_message!, turn, ts, ...over },
});
const answer = (order: number, n: number, text: string, turn: TurnId): ViewEntry => ({
  agent_text: { ...agentText(order, n, text).agent_text!, turn },
});
const ended = (order: number, turn: TurnId, ts: number, over: Partial<StepEntry> = {}): ViewEntry =>
  step(order, { turn, ts, ...over });

/** The question at 4:28 pm and the start of its answer, as turn T1. */
const asked = (lang: Lang): ViewEntry[] => [
  ask(1, 1, say(lang, SAY.question), T1, pm(28)),
  answer(2, 1, say(lang, SAY.answer), T1),
];

/** `n` messages queued behind the turn, from order 10, each queued a minute apart from 4:29 pm. */
const queued = (lang: Lang, n: number): ViewEntry[] =>
  QUEUED.slice(0, n).map((words, i) =>
    ask(10 + i, 10 + i, say(lang, words), queuedTurn(i), pm(29), { status: "queued" }),
  );

/** A thread's state for these screens. */
export type QueueScene = {
  entries: ViewEntry[];
  /** claude-acp by default; codex-acp for ChatGPT. */
  agent?: AgentId;
  status?: ThreadStatus;
  /** HeaderEntry.queue_held: the queue waits for the lawyer (after Stop, a failure or a restart). */
  held?: boolean;
  /** H9's steer_joins_turn: true where a steer joins the running turn (Claude), false on Codex. */
  joins?: boolean;
  /** Commands the host refuses, by name, with a code. */
  refuse?: Partial<Record<"send_prompt" | "remove_queued_prompt", ErrorCode>>;
};

/** A host with one matter and its one thread in `scene`, whose view arrives as the room subscribes. */
export function queueHost(scene: QueueScene): FakeTransport {
  const agent = scene.agent ?? "claude-acp";
  const status = scene.status ?? "running";
  const head = header();
  const h: HeaderEntryH9 = {
    ...head.header!,
    matter: MATTER.matter,
    agent,
    status,
    ...(scene.held === true ? { queue_held: true } : {}),
    ...(scene.joins === undefined ? {} : { steer_joins_turn: scene.joins }),
  };
  head.header = h;
  const refuse = (code: ErrorCode | undefined) => {
    if (code !== undefined)
      throw { code, message: "the review's scripted refusal", retryable: false };
  };
  const transport: FakeTransport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined)
        return {
          threads: [
            { thread: THREAD, matter: MATTER.matter, agent, status, title: "Summoning order" },
          ],
        };
      throw new Error(`the review's host doesn't answer ${JSON.stringify(query)}`);
    },
    dispatch: (envelope): CommandOutcome => {
      const command = envelope.command as Record<string, unknown>;
      if (command["send_prompt"] !== undefined) {
        refuse(scene.refuse?.send_prompt);
        return { prompt_accepted: { turn: T2, message: uuid(99, 5) } };
      }
      if (command["remove_queued_prompt"] !== undefined) refuse(scene.refuse?.remove_queued_prompt);
      return "done";
    },
    subscribe: (stream) =>
      queueMicrotask(() =>
        transport.deliver(event(batch(1, [reset(scene.entries, head)], undefined, stream))),
      ),
  });
  return transport;
}

/** matters/queue: `n` messages queued while the answer runs. */
export const queueScene = (lang: Lang, n: number, over: Partial<QueueScene> = {}): QueueScene => ({
  entries: [...asked(lang), ...queued(lang, n)],
  joins: true,
  ...over,
});

/** matters/held and held-failed: the answer stopped (or failed) at 4:32 pm with three messages queued. */
export const heldScene = (lang: Lang, how: "stopped" | "failed"): QueueScene => ({
  entries: [
    ...asked(lang),
    ended(3, T1, pm(32), { stop: how === "stopped" ? "cancelled" : "failed" }),
    ...queued(lang, 3),
  ],
  status: "idle",
  held: true,
});

/** matters/steer-claude, after Send now: the words stand in the running answer, taken in at 4:31 pm. */
export const steerMarkScene = (lang: Lang): QueueScene => ({
  entries: [
    ...asked(lang),
    ask(3, 2, say(lang, SAY.steerClaude), T1, pm(31), {
      delivery: "steer",
      steer: { injected: { turn: T1 } },
    }),
    answer(4, 3, say(lang, SAY.boardMinutes), T1),
  ],
  joins: true,
});

/** matters/steer-claude, the answer ending as the words came: they head the next exchange and say why. */
export const requeuedScene = (lang: Lang): QueueScene => ({
  entries: [
    ...asked(lang),
    ended(3, T1, pm(31)),
    ask(4, 2, say(lang, SAY.steerClaude), T2, pm(31), {
      delivery: "steer",
      steer: { requeued: { turn: T2 } },
    }),
    answer(5, 3, say(lang, SAY.boardMinutes), T2),
  ],
  joins: true,
});

/** matters/steer-chatgpt: the cheque's question answered at 4:15 pm, and the mandate's question running. */
const mandate = (lang: Lang): ViewEntry[] => [
  ask(1, 1, say(lang, SAY.chequeQuestion), T1, pm(14)),
  answer(2, 1, say(lang, SAY.chequeAnswer), T1),
];

/** matters/steer-chatgpt before Send now: Codex, whose answer runs. */
export const chatgptScene = (lang: Lang): QueueScene => ({
  entries: mandate(lang),
  agent: "codex-acp",
  joins: false,
});

/** matters/steer-chatgpt after Send now: the answer stopped at 4:32 pm and kept, the words sent first. */
export const sentNowScene = (lang: Lang): QueueScene => ({
  entries: [
    ...mandate(lang),
    ask(3, 2, say(lang, SAY.steerChatgpt), T2, pm(32), {
      delivery: "steer",
      steer: { sent_now: { turn: T2 } },
    }),
    ended(4, T1, pm(32), { stop: "cancelled" }),
    answer(5, 3, say(lang, SAY.afterSteer), T2),
  ],
  agent: "codex-acp",
  joins: false,
});

/** matters/steer-aside: a search the 4:31 pm steer moved to the background, finished at 4:32 pm, the answer done. */
export const asideScene = (lang: Lang): QueueScene => {
  const background: BackgroundMark = { steer: uuid(2, 5), finished_at: pm(32) };
  const search: ToolCallEntryH9 = {
    order: 3,
    tool_call: uuid(1, 7),
    turn: T1,
    title: say(lang, SAY.search),
    kind: "search",
    status: "completed",
    locations: [],
    content: [],
    background,
  };
  return {
    entries: [
      ask(1, 1, say(lang, SAY.question), T1, pm(28)),
      answer(2, 1, say(lang, SAY.answer), T1),
      { tool_call: search },
      ask(4, 2, say(lang, SAY.steerAside), T1, pm(31), {
        delivery: "steer",
        steer: { injected: { turn: T1 } },
      }),
      answer(5, 3, say(lang, SAY.boardMinutes), T1),
      ended(6, T1, pm(33)),
    ],
    status: "idle",
    joins: true,
  };
};

/** The words the steer screens type into the field. */
export const steerWords = {
  claude: (lang: Lang) => say(lang, SAY.steerClaude),
  chatgpt: (lang: Lang) => say(lang, SAY.steerChatgpt),
};
