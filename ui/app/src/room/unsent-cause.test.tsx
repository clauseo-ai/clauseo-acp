// @vitest-environment happy-dom
// Why a message never started, as the host says it (#1314: TurnEnded.cause, carried on the user-message entry as
// `unsent`, read through a stand-in until that binding), in the real room over FakeTransport: each cause's words, and
// whether the words come back to the box. Unknown or absent gives today's neutral line.
import { afterEach, expect, test } from "vitest";
import { agentText, reset, THREAD, upsert, userMessage, uuid } from "../testing/stream-script.ts";
import type { ThreadStatus, ThreadSummary, ViewEntry } from "../transport/protocol.ts";
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
/** The later message: sent, or ended with a status and cause the bindings don't carry yet (#1314's stand-ins). */
const later = (status: string, unsent?: string): ViewEntry => {
  const e = userMessage(3, 2, LATER);
  const m = {
    ...e.user_message!,
    status,
    turn: uuid(2, 4),
    ...(unsent === undefined ? {} : { unsent }),
  };
  return { user_message: m as NonNullable<ViewEntry["user_message"]> };
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
async function endsWith(status: string, cause?: string, extra: ViewEntry[] = []) {
  h = await mount();
  h.deliver([reset([...ANSWERED, later("sent")], headerOf({ queued: { behind: THREAD } }))]);
  await h.settle();
  h.deliver([upsert(later(status, cause)), ...extra.map(upsert), upsert(headerOf("idle"))]);
  await h.settle();
}

test("stopped: the words come back, and the line says you stopped it", async () => {
  await endsWith("removed", "stopped");
  expect(field().value).toBe(LATER);
  expect(line()).toBe("Not sent, because you stopped it.");
});

test("sent_now: the words come back, and the line says a newer message went first", async () => {
  await endsWith("not_sent", "sent_now");
  expect(field().value).toBe(LATER);
  expect(line()).toBe("Not sent: you sent a newer message first.");
});

test("not_sent: the words come back, and the line says the answer it was for stopped", async () => {
  await endsWith("not_sent", "not_sent");
  expect(field().value).toBe(LATER);
  expect(line()).toBe("Not sent, because the answer it was for stopped.");
});

test("a message not sent is hidden from the transcript: its words are in the box, not shown as a question", async () => {
  await endsWith("not_sent", "sent_now");
  const shown = [...h!.host.querySelectorAll("[data-entry-id]")].map((n) =>
    n.getAttribute("data-entry-id"),
  );
  expect(shown.some((id) => id?.includes(uuid(2, 5)))).toBe(false);
});

test("removed, sign_in and update: nothing comes back and no line shows", async () => {
  for (const cause of ["removed", "sign_in", "update"]) {
    await endsWith("removed", cause);
    expect(field().value, cause).toBe("");
    expect(h!.one("unsent"), cause).toBeNull();
    await h!.unmount();
    h = null;
  }
});

test("update: the queued copy's strip says it waits because Clauseo was updating", async () => {
  const copy = userMessage(4, 3, LATER);
  await endsWith("removed", "update", [
    { user_message: { ...copy.user_message!, status: "queued", turn: uuid(3, 4) } },
  ]);
  expect(h!.one("queue-why")?.textContent).toBe(
    "This waits because Clauseo was updating. It goes once Clauseo is ready.",
  );
});

test("an unknown cause reads as none: today's neutral line", async () => {
  await endsWith("removed", "a_cause_from_later");
  expect(field().value).toBe(LATER);
  expect(line()).toBe("This message wasn't sent. It's back in the box below.");
});

test("with a cause, this window's Stop record doesn't word the line", async () => {
  h = await mount();
  h.deliver([reset([...ANSWERED, later("sent")], headerOf({ queued: { behind: THREAD } }))]);
  await h.settle();
  await h.click(h.one("cancel"));
  h.deliver([upsert(later("not_sent", "sent_now")), upsert(headerOf("idle"))]);
  await h.settle();
  expect(line()).toBe("Not sent: you sent a newer message first.");
});
