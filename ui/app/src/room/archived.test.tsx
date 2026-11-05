// @vitest-environment happy-dom
// An archived conversation is read only (archived.tsx), in the real room over FakeTransport: opened from Find's archived
// result, its entries show and one quiet line stands in the composer's place, with no field, Send or queue; a live
// conversation opened after it brings the composer back; nothing typed or pressed sends a question to it, which the
// host would refuse ("the thread is archived"); it offers neither Retry from here nor Restart, which the host refuses
// too; and axe finds nothing in either theme. happy-dom computes no styles, so colour contrast is checked by hand
// (window.__clauseoAxe in the review), as for the room's own axe test.
import axe from "axe-core";
import { act } from "react";
import { iconIn } from "@clauseo/components/icons/icon-in.tsx";
import { IconArchived } from "@clauseo/components/icons/icons.tsx";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import {
  agentText,
  MATTER,
  reset,
  step,
  THREAD,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type { HostQuery, QueryResult, SearchHit, ViewEntry } from "../transport/protocol.ts";
import { Conversations } from "./conversations.ts";
import { mountRoom, summary, THE_MATTER, type Harness, type Script } from "./thread/harness.ts";

const NOW = Date.UTC(2026, 10, 4, 11, 0);
const T3 = uuid(3, 1);

let h: Harness | null = null;
beforeAll(() => {
  document.title = "Clauseo";
});
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  Element.prototype.scrollIntoView = () => {};
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

/** Find's one result: an answer in the archived conversation T3, which the host's list leaves out. */
const ARCHIVED_HIT: SearchHit = {
  kind: "answer",
  matter: MATTER,
  matter_title: THE_MATTER.name,
  thread: T3,
  thread_title: "Old bail order",
  order: 3,
  at: NOW,
  snippet: [{ text: "bail", hit: true }],
};
const search = (q: HostQuery): QueryResult => {
  if (typeof q === "object" && "search" in q)
    return { search: { page: { hits: [ARCHIVED_HIT], mode: "ranked" } } };
  throw new Error("not scripted");
};

const line = () => h?.one("archived-line") ?? null;
const composer = () => h?.one("composer") ?? null;

/** The room with its one live conversation open, then Find's archived result pressed and its thread's window given. */
async function openArchived(
  display: Script["display"] = { language: "en" },
  next?: Script["next"],
  entries: ViewEntry[] = [],
) {
  h = await mountRoom({
    threads: [summary()],
    query: search,
    display,
    ...(next === undefined ? {} : { next }),
  });
  await h.deliverAwaited([reset([userMessage(2, 2, "Draft the surety affidavit")])]);
  await h.settle();
  expect(composer()).not.toBeNull();
  await act(async () => {
    const input = document.querySelector<HTMLInputElement>('[data-testid="find"]');
    if (input === null) throw new Error("no Find");
    input.focus();
    Reflect.set(HTMLInputElement.prototype, "value", "bail", input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => settled());
  await h.settle();
  await h.click(h.one("find-conversation"));
  h.deliverOn(`thread:${T3}`, [
    reset(
      [userMessage(2, 2, "Was bail refused?"), agentText(3, 3, "Yes, on 2 March."), ...entries],
      {
        header: {
          order: 1,
          thread: T3,
          matter: MATTER,
          agent: "claude-acp",
          status: "archived",
          config_options: [],
          commands: [],
          plan: [],
        },
      },
    ),
  ]);
  await h.settle();
}

test("an archived conversation opened from Find is read only: its entries show, and one line stands in the composer's place", async () => {
  await openArchived();
  // the thread stays readable, in its scroller
  expect(h?.all("message-user").map((m) => m.textContent)).toEqual([
    expect.stringContaining("Was bail refused?"),
  ]);
  expect(document.querySelector('[data-testid="transcript"] .room-thread')).not.toBeNull();
  // the line: the archive glyph at 16 px, then the words
  expect(line()?.textContent).toBe("Archived. Read only.");
  expect(iconIn(line(), IconArchived, 16)).not.toBeNull();
  expect(line()?.firstElementChild?.tagName.toLowerCase()).toBe("svg");
  // no field, no Send, no queue strip: no composer at all
  expect(composer()).toBeNull();
  expect(h?.one("send")).toBeNull();
  expect(document.querySelector("form[data-composer]")).toBeNull();
  expect(document.querySelector(".room-queue")).toBeNull();
});

test("a live conversation opened after the archived one brings the composer back", async () => {
  await openArchived();
  expect(line()).not.toBeNull();
  const live = h?.all("conversation").find((c) => c.textContent?.includes(summary().title ?? ""));
  await h?.click(live ?? h?.all("conversation")[0]);
  await h?.settle();
  expect(line()).toBeNull();
  expect(composer()).not.toBeNull();
  expect(h?.one("send")).not.toBeNull();
});

test("nothing typed or pressed in an archived conversation sends it a question", async () => {
  await openArchived();
  // whatever field stands in the work column, words typed and Enter pressed, and any form there submitted
  for (const field of document.querySelectorAll<HTMLTextAreaElement>(".room-centre textarea")) {
    await act(async () => {
      field.focus();
      Reflect.set(HTMLTextAreaElement.prototype, "value", "Was bail granted later?", field);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => {
      field.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
      );
    });
  }
  for (const form of document.querySelectorAll<HTMLFormElement>(".room-centre form"))
    await act(async () => form.requestSubmit());
  await h?.settle();
  expect(h?.sent("send_prompt")).toEqual([]);
  // nor to the live conversation open before it
  expect(h?.sent("send_prompt").some((c) => c.thread === THREAD)).toBe(false);
});

/** The host offers Retry from here (H13) and Restart (H10), and refuses both in an archived conversation. */
const RETRY_RESTART: Script["next"] = {
  parts: ["retry", "restart"],
  query: () => ({ threads: [] }),
};

test("an archived conversation offers no Retry from here, which a live one opened after it does", async () => {
  // the room makes its Conversations itself: catch it as it starts (the spy calls through)
  const start = vi.spyOn(Conversations.prototype, "start");
  await openArchived(undefined, RETRY_RESTART);
  const talks = start.mock.contexts[0] as Conversations | undefined;
  expect(line()).not.toBeNull();
  expect(talks?.view().canRetry).toBe(false);
  const live = h?.all("conversation").find((c) => c.textContent?.includes(summary().title ?? ""));
  await h?.click(live ?? h?.all("conversation")[0]);
  await h?.settle();
  expect(talks?.view().canRetry).toBe(true);
  start.mockRestore();
});

test("an archived conversation whose turn was interrupted offers no Restart", async () => {
  await openArchived(undefined, RETRY_RESTART, [
    step(9, { stop: "failed", failure: "thread.interrupted" }),
  ]);
  expect(h?.one("restart")).toBeNull();
  expect(line()).not.toBeNull();
});

const violations = async () =>
  (await axe.run(document, { resultTypes: ["violations"] })).violations.map(
    (v) => `${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
  );

for (const theme of ["light", "dark"] as const) {
  test(`axe finds no violation in an archived conversation: ${theme}`, async () => {
    await openArchived({ theme, language: "en" });
    expect(line()).not.toBeNull();
    expect(document.documentElement.dataset["theme"]).toBe(theme === "dark" ? "night" : "day");
    expect(await violations()).toEqual([]);
  }, 20_000);
}
