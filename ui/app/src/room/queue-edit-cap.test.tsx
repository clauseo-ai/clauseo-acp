// @vitest-environment happy-dom
// A queued message's Edit over the host's cap on a prompt's text (queue-strip.tsx, prompt-cost.ts): Save sends nothing,
// the edit opens again with the lawyer's words, and the strip says prompt.too_long's words, as it says the host's own
// prompt.too_long; at the cap the edit goes. Apart from queue-strip.test.tsx, which is at the size limit.
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { StoreContext } from "../store/hooks.ts";
import { header, reset, STREAM, THREAD, userMessage, uuid } from "../testing/stream-script.ts";
import type { CommandOutcome, HostCommand, StreamId, ViewEntry } from "../transport/protocol.ts";
import { loadQueueStrip } from "./queue-chunk.ts";
import { QueueSlot } from "./queue-slot.tsx";
import { defaultOutcome, mountRoom, type Harness } from "./thread/harness.ts";
import { MAX_PROMPT_TEXT_BYTES } from "./thread/prompt-cost.ts";

let h: Harness | null = null;
let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  await h?.unmount();
  h = null;
});

async function mount(dispatch?: (c: HostCommand) => CommandOutcome | Promise<CommandOutcome>) {
  h = await mountRoom(dispatch === undefined ? {} : { dispatch });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const transport = h.transport;
  await act(async () =>
    root!.render(
      createElement(
        StoreContext,
        { value: h!.store },
        createElement(QueueSlot, {
          transport,
          stream: STREAM as StreamId,
          thread: THREAD,
          onFocusField: () => {},
        }),
      ),
    ),
  );
  await act(async () => void (await loadQueueStrip()));
  const queued: ViewEntry = {
    user_message: { ...userMessage(5, 2, "A.").user_message!, status: "queued", turn: uuid(2, 4) },
  };
  h.deliver([reset([queued], header("running"))]);
  await h.settle();
}

const txt = (n: Element | null | undefined) => (n?.textContent ?? "").replace(/\u00a0/g, " ");
const errorLine = () => host!.querySelector<HTMLElement>('[data-testid="queue-error"]');
const editField = () => host!.querySelector<HTMLTextAreaElement>(".queue-edit");

/** Opens the row's edit, puts `text` in it, and presses Save. */
async function edit(text: string) {
  if (editField() === null)
    await h!.click(host!.querySelector('[data-testid="queued"] [data-testid="queued-edit"]'));
  const field = editField()!;
  await act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", text, field);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await h!.click([...host!.querySelectorAll("button")].find((b) => txt(b) === "Save"));
  await act(async () => h!.store.flush());
}

test("an edit over the cap sends nothing, opens again with its words and says why; at the cap it goes", async () => {
  await mount();
  const over = "a".repeat(MAX_PROMPT_TEXT_BYTES - 1) + "\n";
  await edit(over);
  expect(h!.sent("edit_queued_prompt")).toEqual([]);
  expect(editField()?.value).toBe(over);
  expect(errorLine()?.textContent).toBe(
    "This question is too long to send. Save the text as a file in the matter's folder, then ask about it. Your words are kept.",
  );
  const atCap = "a".repeat(MAX_PROMPT_TEXT_BYTES);
  await edit(atCap);
  expect(h!.sent("edit_queued_prompt")).toEqual([
    { thread: THREAD, turn: uuid(2, 4), text: atCap },
  ]);
  expect(errorLine()).toBeNull();
});

test("the host's own prompt.too_long on an edit says the same words", async () => {
  await mount((c) => {
    if (c.edit_queued_prompt !== undefined)
      throw { code: "prompt.too_long", message: "raw host text", retryable: false };
    return defaultOutcome(c);
  });
  await edit("A, and the deed.");
  expect(h!.sent("edit_queued_prompt")).toHaveLength(1);
  expect(editField()?.value).toBe("A, and the deed.");
  expect(errorLine()?.textContent).toBe(errorWords("en", "prompt.too_long"));
});
