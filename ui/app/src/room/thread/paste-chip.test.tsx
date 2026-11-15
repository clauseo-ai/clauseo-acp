// @vitest-environment happy-dom
// The long paste's chips in the room (composer.tsx, paste-chips.tsx): what goes is one string, the typed words then
// each chip's text; the cap counts the chips, the window's (prompt-cost.ts) and the host's (ChatGPT's, #985) alike, and
// a refusal keeps them; and the chips come back with the composer's words wherever those come back: the draft after a
// visit to another conversation, and the stopped question's refill.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import {
  forgetChipsForTests,
  PASTE_CHIP_MIN,
} from "@clauseo/components/clauseo/paste-chips-logic.ts";
import { reset } from "../../testing/stream-script.ts";
import type { EntryId, HostError, MatterId } from "../../transport/protocol.ts";
import { defaultOutcome, mountRoom, type Harness } from "./harness.ts";
import { MAX_PROMPT_TEXT_BYTES } from "./prompt-cost.ts";
import type { Drafts } from "./session.ts";
import { WorkComposer } from "./work-composer.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let h: Harness | null = null;
let root: Root | null = null;
let host: HTMLDivElement | null = null;
beforeEach(() => forgetChipsForTests());
afterEach(async () => {
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

test("Enter sends the typed words, then the chips in paste order, as one text block; the accepted send empties all", async () => {
  h = await mountRoom();
  h.deliver([reset([])]);
  const one = brief(PASTE_CHIP_MIN, "a");
  const two = brief(PASTE_CHIP_MIN, "b");
  await paste(one);
  await paste(two);
  await type("Which brief is stronger?");
  expect(chips()).toBe(2);
  await h.key({ key: "Enter", keyCode: 13 });
  expect(sentText()).toEqual([`Which brief is stronger?\n\n${one}\n\n${two}`]);
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
  expect(sentText()).toEqual([`Summarise it\n\n${one}`]);
  expect(h.one("error")?.dataset["code"]).toBe("prompt.too_long");
  expect(chips()).toBe(1);
  expect(field().value).toBe("Summarise it");
});

const A = "01900002-0000-7000-8000-00000000000a" as MatterId;
const B = "01900002-0000-7000-8000-00000000000b" as MatterId;

/** WorkComposer as room.tsx renders it, keyed by its conversation; `refill` as work.tsx passes the stopped question. */
function room(drafts: Drafts) {
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
          onAsk={async () => true}
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
  expect(drafts.get(A)).toBe(`Compare.\n\n${one}\n\n${two}`);
  await act(async () => document.querySelector<HTMLButtonElement>(".q-paste button")!.click());
  await show(B);
  await show(A);
  expect(chips()).toBe(1);
  expect(drafts.get(A)).toBe(`Compare.\n\n${two}`);
});

test("the stopped question's refill brings its chips back with its words", async () => {
  const drafts: Drafts = new Map();
  const show = room(drafts);
  await show(A);
  const one = brief(PASTE_CHIP_MIN, "a");
  await paste(one);
  await type("Read this.");
  const asked = `Read this.\n\n${one}`;
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
  expect(drafts.get(A)).toBe(asked);
});
