// @vitest-environment happy-dom
// Why a message never started, as the host says it (TurnEnded.cause, carried on the user-message entry as
// `unsent`; the status stays removed), in the real room over FakeTransport: each cause's words, and whether the words come back to the box. A sign-in or an update queues a held
// copy, so its words stay out of the box; every other cause, and none, gives them back. Unknown or absent gives
// today's line.
import { afterEach, expect, test } from "vitest";
import { agentText, reset, THREAD, upsert, userMessage, uuid } from "../testing/stream-script.ts";
import type {
  MessageId,
  ThreadStatus,
  ThreadSummary,
  UnsentCause,
  UserMessageStatus,
  ViewEntry,
} from "../transport/protocol.ts";
import { fakeNext } from "./fake-next.ts";
import { mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";
import { forgetSteerRestores } from "./thread/steer-restore.ts";
import { forgetUnsent } from "./thread/unsent-restore.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  forgetSteerRestores();
  forgetUnsent();
});

const row = (thread: string, status: ThreadStatus, title: string): ThreadSummary => ({
  thread,
  matter: THE_MATTER.matter,
  agent: "claude-acp",
  status,
  title,
});
const headerOf = (status: ThreadStatus): ViewEntry => ({
  header: {
    order: 1,
    thread: THREAD,
    matter: THE_MATTER.matter,
    agent: "claude-acp",
    status,
    config_options: [],
    commands: [],
    plan: [],
  },
});
const mount = () =>
  mountRoom({
    threads: [row(THREAD, "idle", "Lease notice")],
    next: fakeNext({ matters: () => [], parts: ["app"] }),
  });

const LATER = "And the sublet clause in the renewal?";
/** The later message: sent, or ended with `status` and `unsent`, a newer host's value included (read as a string,
 * as the wire may carry it). */
const later = (status: UserMessageStatus, unsent?: string): ViewEntry => {
  const e = userMessage(3, 2, LATER);
  const m = {
    ...e.user_message!,
    status,
    turn: uuid(2, 4),
    ...(unsent === undefined ? {} : { unsent: unsent as UnsentCause }),
  };
  return { user_message: m };
};
const ANSWERED = [
  userMessage(1, 1, "Read clause nine."),
  agentText(2, 1, "Clause nine says notice."),
];
const field = () => h!.one("composer") as HTMLTextAreaElement;
const line = () =>
  h!
    .one("unsent")
    ?.textContent?.replace(/\u00a0/g, " ")
    .trim();

/** THREAD with an answer and its later message waiting; then the host ends it with `status` and `cause`. */
async function endsWith(status: UserMessageStatus, cause?: string, extra: ViewEntry[] = []) {
  h = await mount();
  h.deliver([reset([...ANSWERED, later("sent")], headerOf({ queued: { behind: THREAD } }))]);
  await h.settle();
  h.deliver([upsert(later(status, cause)), ...extra.map(upsert), upsert(headerOf("idle"))]);
  await h.settle();
}

const STOPPED =
  "Not sent, because you stopped it before it started. Your message is back in the box below.";
const PLAIN = "This message wasn't sent. It's back in the box below.";
const SENT_NOW = "Not sent: you sent a newer message first. It's back in the box below.";
const NOT_SENT = "Not sent: the answer it was for had stopped. It's back in the box below.";
const UPDATE =
  "An update was running. This message waits in the queue and goes once the update is done.";
const SIGN_IN =
  "You were signed out of Claude. This message waits in the queue and goes once you're signed in.";

test.each([
  ["stopped", STOPPED],
  ["removed", PLAIN],
  ["sent_now", SENT_NOW],
  ["not_sent", NOT_SENT],
])("%s: the words come back to the box, and the line says why", async (cause, said) => {
  await endsWith("removed", cause);
  expect(field().value).toBe(LATER);
  expect(line()).toBe(said);
});

test("removed: the line puts no blame on the lawyer", async () => {
  await endsWith("removed", "removed");
  expect(line()).not.toContain("you stopped");
});

test("no cause (an older log): today's line, and the words come back", async () => {
  await endsWith("removed");
  expect(field().value).toBe(LATER);
  expect(line()).toBe(PLAIN);
});

test("an unknown cause reads as none: today's line, and the words come back", async () => {
  await endsWith("removed", "a_cause_from_later");
  expect(field().value).toBe(LATER);
  expect(line()).toBe(PLAIN);
});

test("sent_now: the removed message isn't shown as a question, its words are in the box", async () => {
  await endsWith("removed", "sent_now");
  expect(field().value).toBe(LATER);
  expect(line()).toBe(SENT_NOW);
  const shown = [...h!.host.querySelectorAll("[data-entry-id]")].map((n) =>
    n.getAttribute("data-entry-id"),
  );
  expect(shown.some((id) => id?.includes(uuid(2, 5)))).toBe(false);
});

/** The host's queued copy of the later message, marked on its own entry (copy_of); `of` is the message it names,
 * the later message by default. */
const copy = (cause?: UnsentCause, of: MessageId = uuid(2, 5)): ViewEntry => {
  const e = userMessage(4, 3, LATER);
  const m = {
    ...e.user_message!,
    status: "queued" as const,
    turn: uuid(3, 4),
    ...(cause === undefined ? {} : { copy_of: { message: of, cause } }),
  };
  return { user_message: m };
};
const why = () => h!.one("queue-why")?.textContent?.replace(/\u00a0/g, " ");

test.each(["sign_in", "update"] as const)(
  "%s: the host queues a copy, so the words don't also come back to the box",
  async (cause) => {
    await endsWith("removed", cause, [copy(cause)]);
    expect(field().value).toBe("");
    expect(h!.one("unsent")).toBeNull();
  },
);

test.each(["sign_in", "update"] as const)(
  "%s with its copy not yet in: still nothing in the box",
  async (cause) => {
    await endsWith("removed", cause);
    expect(field().value).toBe("");
    expect(h!.one("unsent")).toBeNull();
  },
);

test("an original a queued copy names stays out of the box, whatever its own cause says", async () => {
  await endsWith("removed", undefined, [copy("update")]);
  expect(field().value).toBe("");
  expect(h!.one("unsent")).toBeNull();
});

test("a copy marked for an update: the strip says an update was running, naming neither Clauseo nor the agent", async () => {
  await endsWith("removed", "update", [copy("update")]);
  expect(why()).toBe(UPDATE);
});

test("a copy marked for a sign-in: the strip names the agent", async () => {
  await endsWith("removed", "sign_in", [copy("sign_in")]);
  expect(why()).toBe(SIGN_IN);
});

test("a copy whose original is missing from the log: the strip still says why, and nothing comes back", async () => {
  h = await mount();
  h.deliver([
    reset([...ANSWERED, copy("sign_in", uuid(9, 5))], headerOf({ queued: { behind: THREAD } })),
  ]);
  await h.settle();
  expect(why()).toBe(SIGN_IN);
  expect(field().value).toBe("");
  expect(h.one("unsent")).toBeNull();
});

test("no copy_of, no line: not even right after a message that ended for an update", async () => {
  await endsWith("removed", "update", [copy()]);
  expect(h!.one("queue-why")).toBeNull();
});

test("with a cause, this window's Stop record doesn't word the line", async () => {
  h = await mount();
  h.deliver([reset([...ANSWERED, later("sent")], headerOf({ queued: { behind: THREAD } }))]);
  await h.settle();
  await h.click(h.one("cancel"));
  h.deliver([upsert(later("removed", "sent_now")), upsert(headerOf("idle"))]);
  await h.settle();
  expect(line()).toBe(SENT_NOW);
});
