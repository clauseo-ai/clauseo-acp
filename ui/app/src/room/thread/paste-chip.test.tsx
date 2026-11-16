// @vitest-environment happy-dom
// The long paste's chips in the room (composer.tsx, paste-chips.tsx): what goes is one string, each chip's text in
// paste order, as they are drawn, then the typed words; the cap counts the chips, the window's (prompt-cost.ts) and the host's (ChatGPT's, #985) alike, and
// a refusal keeps them; and the chips come back with the composer's words wherever those come back, as they were: the
// draft after a visit to another conversation (the typed words and the chips' texts kept apart), and the stopped
// question's refill.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import {
  forgetChipsForTests,
  PASTE_CHIP_MIN,
} from "@clauseo/components/clauseo/paste-chips-logic.ts";
import { header, reset, upsert, userMessage, uuid } from "../../testing/stream-script.ts";
import type {
  CommandOutcome,
  EntryId,
  HostCommand,
  HostError,
  MatterId,
  ViewEntry,
} from "../../transport/protocol.ts";
import { loadQueueStrip } from "../queue-chunk.ts";
import { defaultOutcome, mountRoom, type Harness } from "./harness.ts";
import { MAX_PROMPT_TEXT_BYTES } from "./prompt-cost.ts";
import type { Drafts } from "./session.ts";
import { forgetSteerRestores } from "./steer-restore.ts";
import { WorkComposer } from "./work-composer.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let h: Harness | null = null;
let root: Root | null = null;
let host: HTMLDivElement | null = null;
beforeEach(() => forgetChipsForTests());
afterEach(async () => {
  forgetSteerRestores();
  await h?.unmount();
  h = null;
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const field = () => document.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
const chips = () => document.querySelectorAll('[data-testid="paste-chip"]').length;
const brief = (n: number, mark = "a") => `${mark}word `.repeat(Math.ceil(n / 6)).slice(0, n);
const type = (text: string) =>
  act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", text, field());
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });
async function paste(text: string) {
  const e = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(e, "clipboardData", { value: { getData: () => text } });
  await act(async () => void field().dispatchEvent(e));
}
const sentText = () =>
  h!.sent("send_prompt").map((s) => (s.content[0] as { text: { text: string } }).text.text);

test("Enter sends the chips in paste order, then the typed words, as one text block; the accepted send empties all", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  const one = brief(PASTE_CHIP_MIN, "a");
  const two = brief(PASTE_CHIP_MIN, "b");
  await paste(one);
  await paste(two);
  await type("Which brief is stronger?");
  expect(chips()).toBe(2);
  await h.key({ key: "Enter", keyCode: 13 });
  expect(sentText()).toEqual([`${one}\n\n${two}\n\nWhich brief is stronger?`]);
  expect(h.sent("send_prompt")[0]!.content).toHaveLength(1);
  expect(chips()).toBe(0);
  expect(field().value).toBe("");
});

test("the window's cap counts the chips: over it nothing goes, prompt.too_long is said, and the chips stay", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  // the chip at the cap less the blank line and one typed byte: the typed "ab" puts it one byte over
  const typed = "ab";
  await paste("a".repeat(MAX_PROMPT_TEXT_BYTES - 4 - 1));
  await type(typed);
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.sent("send_prompt")).toEqual([]);
  expect(h.one("error")?.dataset["code"]).toBe("prompt.too_long");
  expect(chips()).toBe(1);
  expect(field().value).toBe(typed);
  // one typed byte fewer is at the cap, and goes
  await type("a");
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.sent("send_prompt")).toHaveLength(1);
});

test("the host's own prompt.too_long (ChatGPT's cap) keeps the chips and the words", async () => {
  const refused: HostError = {
    code: "prompt.too_long",
    message: "for logs only",
    retryable: false,
  };
  h = await mountRoom({
    dispatch: (command) => {
      if (command.send_prompt !== undefined) throw refused;
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([])]);
  const one = brief(PASTE_CHIP_MIN);
  await paste(one);
  await type("Summarise it");
  await h.key({ key: "Enter", keyCode: 13 });
  expect(sentText()).toEqual([`${one}\n\nSummarise it`]);
  expect(h.one("error")?.dataset["code"]).toBe("prompt.too_long");
  expect(chips()).toBe(1);
  expect(field().value).toBe("Summarise it");
});

const A = "01900002-0000-7000-8000-00000000000a" as MatterId;
const B = "01900002-0000-7000-8000-00000000000b" as MatterId;

/** WorkComposer as room.tsx renders it, keyed by its conversation; `refill` as work.tsx passes the stopped question. */
function room(drafts: Drafts, sent: string[] = []) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  return (key: MatterId, refill: { key: EntryId; text: string } | null = null) =>
    act(async () =>
      root!.render(
        <WorkComposer
          key={key}
          drafts={drafts}
          draftKey={key}
          commands={[]}
          disabled={false}
          busy={false}
          refused={null}
          failed={null}
          refill={refill}
          onAsk={async (text) => {
            sent.push(text);
            return true;
          }}
          onStop={() => {}}
        />,
      ),
    );
}

test("a conversation's chips are in its draft: there again after a visit to another, removed ones gone", async () => {
  const drafts: Drafts = new Map();
  const show = room(drafts);
  await show(A);
  const one = brief(PASTE_CHIP_MIN, "a");
  const two = brief(PASTE_CHIP_MIN, "b");
  await paste(one);
  await paste(two);
  await type("Compare.");
  await show(B);
  expect(chips()).toBe(0);
  await show(A);
  expect(chips()).toBe(2);
  expect(field().value).toBe("Compare.");
  expect(drafts.get(A)).toEqual({ typed: "Compare.", pastes: [one, two] });
  await act(async () => document.querySelector<HTMLButtonElement>(".q-paste button")!.click());
  await show(B);
  await show(A);
  expect(chips()).toBe(1);
  expect(drafts.get(A)).toEqual({ typed: "Compare.", pastes: [two] });
});

test("the stopped question's refill brings its chips back with its words", async () => {
  const drafts: Drafts = new Map();
  const show = room(drafts);
  await show(A);
  const one = brief(PASTE_CHIP_MIN, "a");
  await paste(one);
  await type("Read this.");
  const asked = `${one}\n\nRead this.`;
  // sent and accepted: the composer empties
  await act(async () => {
    field().dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", keyCode: 13, bubbles: true, cancelable: true }),
    );
  });
  expect(chips()).toBe(0);
  // its turn stops: the question comes back, as the host's transcript holds it
  await show(A, { key: "01900002-0000-7000-8000-0000000000e1" as EntryId, text: asked });
  expect(chips()).toBe(1);
  expect(field().value).toBe("Read this.");
  expect(drafts.get(A)).toEqual({ typed: "Read this.", pastes: [one] });
});

const enter = () =>
  act(async () => {
    field().dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", keyCode: 13, bubbles: true, cancelable: true }),
    );
  });

test("a chip whose text holds another chip's comes back byte for byte after a visit, and goes chips first, as pasted", async () => {
  const drafts: Drafts = new Map();
  const sent: string[] = [];
  const show = room(drafts, sent);
  await show(A);
  // agreement A, then schedule B pasted out of it: A = P + B + S
  const schedule = brief(25_000, "s");
  const agreement = `${brief(30_000, "p")}${schedule}${brief(30_000, "q")}`;
  await paste(agreement);
  await paste(schedule);
  await type("Is B consistent with A?");
  await show(B);
  await show(A);
  expect(chips()).toBe(2);
  expect(field().value).toBe("Is B consistent with A?");
  const back = drafts.get(A)!;
  expect(back.pastes).toHaveLength(2);
  expect(back.pastes[0] === agreement).toBe(true);
  expect(back.pastes[1] === schedule).toBe(true);
  await enter();
  expect(sent).toHaveLength(1);
  expect(sent[0] === `${agreement}\n\n${schedule}\n\nIs B consistent with A?`).toBe(true);
});

/** A running answer that takes words mid-answer: Enter steers. */
const running = (): ViewEntry => ({
  header: { ...header("running").header!, steer_joins_turn: true } as NonNullable<
    ViewEntry["header"]
  >,
});
const accept = (c: HostCommand): CommandOutcome =>
  c.send_prompt !== undefined
    ? { prompt_accepted: { turn: uuid(9, 4), message: uuid(9, 5) } }
    : defaultOutcome(c);
/** The steer as the host lists it, with the words it went with. */
const steerOf = (text: string, over: object = {}): ViewEntry => ({
  user_message: { ...userMessage(2, 2, text).user_message!, delivery: "steer", ...over },
});

test("a steer not sent comes back with its chips: all the chips first, the field's then its own, then the words", async () => {
  h = await mountRoom({ dispatch: accept });
  h.deliver([reset([userMessage(1, 1, "Q")], running())]);
  const one = brief(PASTE_CHIP_MIN, "a");
  await paste(one);
  await type("Also this.");
  await h.key({ key: "Enter", keyCode: 13 });
  const steered = h.sent("send_prompt")[0]!;
  expect(steered.delivery).toBe("steer");
  const asked = sentText()[0]!;
  expect(asked).toBe(`${one}\n\nAlso this.`);
  expect(chips()).toBe(0);
  // the lawyer goes on: a second paste and words of her own
  const two = brief(PASTE_CHIP_MIN, "b");
  await paste(two);
  await type("Mine.");
  // the host lists the steer waiting, then not sent: it comes back
  h.deliver([upsert(steerOf(asked))]);
  await h.settle();
  h.deliver([upsert(steerOf(asked, { steer: { not_sent: { turn: uuid(1, 4) } } }))]);
  await h.settle();
  expect(chips()).toBe(2);
  expect(field().value).toBe("Mine.\nAlso this.");
  h.deliver([reset([userMessage(1, 1, "Q")], header("idle"))]);
  await h.settle();
  await h.key({ key: "Enter", keyCode: 13 });
  expect(sentText()[1] === `${two}\n\n${one}\n\nMine.\nAlso this.`).toBe(true);
});

test("an edit lost as its message went comes back after the field's chips as a chip, before the field's words", async () => {
  h = await mountRoom({
    dispatch: (c) => {
      if (c.edit_queued_prompt !== undefined)
        throw { code: "thread.turn_not_queued", message: "raw host text", retryable: false };
      return accept(c);
    },
  });
  const queuedText = "Check the lease.";
  const queued: ViewEntry = {
    user_message: {
      ...userMessage(5, 2, queuedText).user_message!,
      status: "queued",
      turn: uuid(2, 4),
    },
  };
  h.deliver([reset([userMessage(1, 1, "Q"), queued], running())]);
  await act(async () => void (await loadQueueStrip()));
  await h.settle();
  const one = brief(PASTE_CHIP_MIN, "a");
  await paste(one);
  await type("My own words.");
  // the queued message edited to a long text, refused as it had gone
  const edited = brief(PASTE_CHIP_MIN + 10, "e");
  await h.click(h.all("queued-edit")[0]!);
  const edit = h.host.querySelector<HTMLTextAreaElement>(".queue-edit")!;
  await act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", edited, edit);
    edit.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await h.click([...h.host.querySelectorAll("button")].find((b) => b.textContent === "Save"));
  await act(async () => h!.store.flush());
  expect(chips()).toBe(2);
  expect(field().value).toBe("My own words.");
  h.deliver([reset([userMessage(1, 1, "Q")], header("idle"))]);
  await h.settle();
  await h.key({ key: "Enter", keyCode: 13 });
  expect(sentText()[0] === `${one}\n\n${edited}\n\nMy own words.`).toBe(true);
});

test("after an accepted send, Cmd+Z never brings the sent question's chips back", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  await paste(brief(PASTE_CHIP_MIN, "a"));
  await paste(brief(PASTE_CHIP_MIN, "b"));
  await type("Which is stronger?");
  await h.key({ key: "Enter", keyCode: 13 });
  expect(sentText()).toHaveLength(1);
  expect(chips()).toBe(0);
  for (let i = 0; i < 3; i++) await h.key({ key: "z", metaKey: true });
  expect(chips()).toBe(0);
});
