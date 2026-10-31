// Scripted host traffic for tests: view entries and patch batches as the host encodes them, with ids that are the
// same on every run. Not shipped: only tests import it.
import type {
  HostEvent,
  PatchBatch,
  StepEntry,
  ToolCallEntry,
  ToolCallStatus,
  ViewEntry,
  ViewPatchBody,
} from "../transport/protocol.ts";

/** A v7-shaped UUID from a number, the same on every run. */
export const uuid = (n: number, kind = 0): string =>
  `0190${kind.toString(16).padStart(4, "0")}-0000-7000-8000-${n.toString(16).padStart(12, "0")}`;

export const THREAD = uuid(1, 1);
export const MATTER = uuid(1, 2);
export const STREAM = `thread:${THREAD}`;
export const EPOCH = uuid(1, 3);
export const TURN = uuid(1, 4);

export const header = (status: "idle" | "running" | "needs_you" = "idle"): ViewEntry => ({
  header: {
    order: 1,
    thread: THREAD,
    matter: MATTER,
    agent: "claude",
    status,
    config_options: [],
    commands: [],
    plan: [],
  },
});

export const userMessage = (order: number, n: number, text: string): ViewEntry => ({
  user_message: {
    order,
    message: uuid(n, 5),
    turn: TURN,
    content: [{ text: { text } }],
    status: "sent",
    edited: false,
  },
});

export const agentText = (
  order: number,
  n: number,
  text = "",
  kind: "answer" | "thought" = "answer",
): ViewEntry => ({
  agent_text: { order, message: uuid(n, 6), turn: TURN, kind, text },
});

export const toolCall = (order: number, n: number, status: ToolCallStatus): ViewEntry => {
  const entry: ToolCallEntry = {
    order,
    tool_call: uuid(n, 7),
    turn: TURN,
    title: `Read file ${n}`,
    kind: "read",
    status,
    locations: [],
    content: [],
  };
  return { tool_call: entry };
};

export const step = (order: number, over: Partial<StepEntry> = {}): ViewEntry => ({
  step: {
    order,
    turn: TURN,
    stop: "end_turn",
    files: [],
    skipped: [],
    undone: false,
    ...over,
  },
});

/** One batch of patches at consecutive cursors from `first`. */
export const batch = (
  first: number,
  bodies: ViewPatchBody[],
  epoch = EPOCH,
  stream = STREAM,
): PatchBatch => ({
  stream,
  epoch,
  patches: bodies.map((body, i) => ({ cursor: first + i, body })),
});

export const reset = (entries: ViewEntry[], head: ViewEntry | null = header()): ViewPatchBody => ({
  reset: {
    view_window: {
      ...(head?.header !== undefined ? { header: { header: head.header } } : {}),
      entries,
      has_older: false,
    },
  },
});

export const upsert = (entry: ViewEntry): ViewPatchBody => ({ upsert: { entry } });

export const append = (n: number, fromLen: number, text: string): ViewPatchBody => ({
  append_text: { message_id: uuid(n, 6), target: "agent_text", from_len: fromLen, text },
});

export const event = (b: PatchBatch): HostEvent => ({ patches: b });

const WORDS =
  "the tenant served notice on the bank under clause nine and the bank replied late ".split(" ");

/** The scripted answer: 20 segments, each a new answer message streamed in 25 chunks and then one tool call that goes
 * pending, in progress, completed; 500 text chunks and 20 tool calls in all, then the step. Every element is one patch
 * body, in order, after a `Reset` holding the header and the user's prompt. */
export function scriptedTurn(): {
  bodies: ViewPatchBody[];
  textBytes: number;
  chunks: number;
  tools: number;
} {
  const bodies: ViewPatchBody[] = [];
  let order = 10;
  let textBytes = 0;
  let chunks = 0;
  for (let segment = 0; segment < 20; segment += 1) {
    const message = 100 + segment;
    bodies.push(upsert(agentText(order++, message)));
    let len = 0;
    for (let c = 0; c < 25; c += 1) {
      const text = `${WORDS[(segment * 25 + c) % WORDS.length]} ${WORDS[(segment + c) % WORDS.length]}, `;
      bodies.push(append(message, len, text));
      len += text.length;
      textBytes += new TextEncoder().encode(text).length;
      chunks += 1;
    }
    const tool = 200 + segment;
    const at = order++;
    for (const status of ["pending", "in_progress", "completed"] as const) {
      bodies.push(upsert(toolCall(at, tool, status)));
    }
  }
  bodies.push(upsert(step(order)));
  return { bodies, textBytes, chunks, tools: 20 };
}
