// @vitest-environment happy-dom
// The room's thread over FakeTransport, the composer's side: reopening a thread, the first Ask making exactly one,
// Enter and the input method's Enter, busy and Stop, a refused send, a lost stream with Try again, no matter.
import { act } from "react";
import { streamFailedWords } from "@clauseo/components/lib/words-breaks.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { afterEach, expect, test, vi } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  STREAM,
  THREAD,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type { HostError } from "../../transport/protocol.ts";
import { defaultOutcome, mountRoom, summary, THE_MATTER, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

const refusal = (code: HostError["code"]): HostError => ({
  code,
  message: "for logs only",
  retryable: false,
});
const field = () => h!.one("composer") as HTMLTextAreaElement;

test("reopen: the matter's newest thread that isn't archived, watched, with its question and answer", async () => {
  const archived = uuid(9, 1);
  h = await mountRoom({ threads: [summary(THREAD, "idle"), summary(archived, "archived")] });
  expect(h.transport.only("query")).toContainEqual({
    call: "query",
    query: { list_threads: { matter: THE_MATTER.matter } },
  });
  expect(h.transport.only("subscribe").map((c) => c.stream)).toEqual([STREAM]);
  h.deliver([
    reset([
      userMessage(5, 1, "What does clause nine say?"),
      agentText(6, 1, "It says **notice**."),
    ]),
  ]);
  expect(h.one("message-user")?.textContent).toContain("What does clause nine say?");
  expect(h.one("message-agent")?.querySelector("strong")?.textContent).toBe("notice");
  // the list is a box of its own (the scoreboard measures it), with Thread's scroller inside
  const list = h.one("transcript");
  expect(list?.querySelector(".room-thread")).not.toBeNull();
  expect(list?.classList.contains("contents")).toBe(false);
  expect([...(list?.classList ?? [])]).toEqual(
    expect.arrayContaining(["flex", "flex-1", "min-h-0", "flex-col"]),
  );
  // every row says which entry it is
  expect(h.all("message-user")[0]?.dataset["entryId"]).toBe(`user_message:${uuid(1, 5)}`);
  expect(h.sent("create_thread")).toEqual([]);
});

test("the first Ask makes one thread, even on a double Enter, then sends; the field empties once it is accepted", async () => {
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => (release = resolve));
  h = await mountRoom({
    threads: [],
    dispatch: async (command) => {
      if (command.create_thread !== undefined) await held;
      return defaultOutcome(command);
    },
  });
  expect(h.transport.only("subscribe")).toEqual([]);
  await h.type("Summarise the indemnity clause");
  await h.key({ key: "Enter", keyCode: 13 });
  await h.key({ key: "Enter", keyCode: 13 });
  // still waiting for create_thread: the words stay
  expect(field().value).toBe("Summarise the indemnity clause");
  await act(async () => release());
  await h.settle();
  expect(h.sent("create_thread")).toEqual([{ matter: THE_MATTER.matter, agent: "claude" }]);
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: "Summarise the indemnity clause" } }] },
  ]);
  expect(field().value).toBe("");
  expect(h.transport.only("subscribe").map((c) => c.stream)).toEqual([STREAM]);
});

test("Enter sends, Shift+Enter breaks the line, and an empty field sends nothing", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  expect(await h.key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(h.sent("send_prompt")).toEqual([]);
  await h.type("First line");
  expect(await h.key({ key: "Enter", keyCode: 13, shiftKey: true })).toBe(false);
  expect(h.sent("send_prompt")).toEqual([]);
  expect(await h.key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(h.sent("send_prompt")).toHaveLength(1);
});

test("an input method's Enter commits its candidate and sends nothing; the Enter after compositionend sends", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  await h.type("नमस्ते");
  const el = field();
  // the two Enters below carry isComposing or keyCode 229, which sendsOnEnter reads; between compositionstart and
  // compositionend the composer holds Enter of its own accord too (composer-field.test.tsx covers a plain Enter there)
  await act(async () => {
    el.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
  });
  expect(await h.key({ key: "Enter", keyCode: 229, isComposing: true })).toBe(false);
  // Safari and some Windows IMEs give the committing Enter keyCode 229 without isComposing
  expect(await h.key({ key: "Enter", keyCode: 229 })).toBe(false);
  await act(async () => {
    el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "नमस्ते" }));
  });
  expect(h.sent("send_prompt")).toEqual([]);
  expect(await h.key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: "नमस्ते" } }] },
  ]);
});

test("busy: Stop in Ask's place, Answering. on the working line, Enter sends nothing, and Stop sends cancel", async () => {
  h = await mountRoom();
  h.deliver([reset([userMessage(5, 1, "Q")], header("running"))]);
  expect(h.one("send")).toBeNull();
  expect(h.one("cancel")).not.toBeNull();
  expect(h.one("working")?.textContent).toBe(frame("en", "Answering."));
  expect(h.one("working")?.dataset["kind"]).toBe("wait");
  await h.type("Another question");
  expect(await h.key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(h.sent("send_prompt")).toEqual([]);
  expect(field().value).toBe("Another question");
  await h.click(h.one("cancel"));
  expect(h.sent("cancel")).toEqual([{ thread: THREAD }]);
  // the turn ends: Ask is back, the words still in the field
  h.deliver([upsert(header("idle")), upsert(step(9, { stop: "cancelled" }))]);
  expect(h.one("cancel")).toBeNull();
  expect(h.one("send")).not.toBeNull();
  expect(h.one("working")).toBeNull();
});

test("busy from the send until its step is in the view, whatever the status says", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  await h.type("Q");
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.one("cancel")).not.toBeNull();
  h.deliver([upsert(userMessage(5, 1, "Q")), upsert(step(6))]);
  expect(h.one("cancel")).toBeNull();
  expect(h.one("send")).not.toBeNull();
});

test("queued counts as busy", async () => {
  h = await mountRoom();
  const head = header();
  head.header!.status = { queued: { behind: uuid(3, 1) } };
  h.deliver([reset([], head)]);
  expect(h.one("cancel")).not.toBeNull();
});

test("a refused send shows its code's words on the working line and keeps the words; the next Ask clears it", async () => {
  let refuse = true;
  h = await mountRoom({
    dispatch: (command) => {
      if (command.send_prompt !== undefined && refuse) throw refusal("store.disk_full");
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([])]);
  await h.type("Summarise it");
  await h.key({ key: "Enter", keyCode: 13 });
  const line = h.one("error");
  expect(line?.dataset["code"]).toBe("store.disk_full");
  expect(line?.textContent).toBe(errorWords("en", "store.disk_full"));
  expect(line?.textContent).not.toContain("for logs only");
  expect(field().value).toBe("Summarise it");
  expect(h.one("working")).toBeNull();
  refuse = false;
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.one("error")).toBeNull();
  expect(field().value).toBe("");
});

test("a refused Stop shows on the working line", async () => {
  h = await mountRoom({
    dispatch: (command) => {
      if (command.cancel !== undefined) throw refusal("command.unknown_thread");
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([], header("running"))]);
  await h.click(h.one("cancel"));
  expect(h.one("error")?.dataset["code"]).toBe("command.unknown_thread");
  expect(h.one("cancel")).not.toBeNull();
});

test("a lost stream shows its code's words with Try again, before a refusal, and Try again asks the store again", async () => {
  h = await mountRoom({
    dispatch: (command) => {
      if (command.send_prompt !== undefined) throw refusal("store.io");
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([userMessage(5, 1, "Q"), agentText(6, 1, "Part")])]);
  await h.type("Again");
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.one("error")?.dataset["code"]).toBe("store.io");
  // a retryable code, since Try again shows only for one (U10)
  await act(async () =>
    h!.transport.failStream(STREAM, { ...refusal("agent.exited"), retryable: true }),
  );
  const line = h.one("stream-failed");
  expect(line?.dataset["code"]).toBe("agent.exited");
  expect(line?.textContent).toContain(streamFailedWords("en", "agent.exited", true));
  const again = h.one("retry");
  expect(again?.textContent).toBe(lim("en", "Try again"));
  const retry = vi.spyOn(h.store, "retry");
  await h.click(again);
  expect(retry).toHaveBeenCalledWith(STREAM);
  // the store asked the host again, and the line went back to the refusal
  expect(h.transport.only("subscribe").length).toBeGreaterThanOrEqual(2);
  expect(h.one("error")?.dataset["code"]).toBe("store.io");
  // the answer so far stays
  expect(h.one("message-agent")?.textContent).toBe("Part");
});

test("with no matter the composer is disabled and nothing asks for threads", async () => {
  h = await mountRoom({ matters: [] });
  expect(field().disabled).toBe(true);
  expect((h.one("send") as HTMLButtonElement).disabled).toBe(true);
  expect(h.transport.only("query").some((c) => typeof c.query === "object")).toBe(false);
  expect(h.host.querySelector(".room-line")).toBeNull();
});

test("Ask's button sends as Enter does", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  await h.type("By the button");
  await h.click(h.one("send"));
  expect(h.sent("send_prompt")).toHaveLength(1);
});

test("a sent turn that ends without its step in view stops being busy once the status says nothing runs", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  await h.type("Q");
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.one("cancel")).not.toBeNull();
  h.deliver([upsert(header("running"))]);
  expect(h.one("cancel")).not.toBeNull();
  h.deliver([upsert(header("idle"))]);
  expect(h.one("cancel")).toBeNull();
  expect(h.one("send")).not.toBeNull();
});

test("a Reset whose header says nothing runs ends the sent turn's busy", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  await h.type("Q");
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.one("cancel")).not.toBeNull();
  h.deliver([reset([userMessage(5, 1, "Q")], header("idle"))]);
  expect(h.one("cancel")).toBeNull();
});

test("a lost stream ends the sent turn's busy", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  await h.type("Q");
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.one("cancel")).not.toBeNull();
  await act(async () => h!.transport.failStream(STREAM, refusal("agent.exited")));
  expect(h.one("cancel")).toBeNull();
  expect(h.one("stream-failed")?.dataset["code"]).toBe("agent.exited");
});

test("Stop pressed again while its cancel is on its way sends one cancel", async () => {
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => (release = resolve));
  h = await mountRoom({
    dispatch: async (command) => {
      if (command.cancel !== undefined) await held;
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([], header("running"))]);
  await h.click(h.one("cancel"));
  await h.click(h.one("cancel"));
  expect(h.sent("cancel")).toHaveLength(1);
  await act(async () => release());
  await h.settle();
  await h.click(h.one("cancel"));
  expect(h.sent("cancel")).toHaveLength(2);
});
