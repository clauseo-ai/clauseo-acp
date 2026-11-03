// @vitest-environment happy-dom
// U7 review, the composer's side of the queue in the real room over FakeTransport: after the hint's Send now or Queue
// the focus is back in the field and the emptied words are one step undo takes back (the engine's own editing, as
// composer-field.test.tsx proves it after Ask); and an edit refused as its message, the last queued, had already gone
// comes back into the field, after any words there, with the working line saying the message had gone.
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { header, reset, upsert, userMessage, uuid } from "../../testing/stream-script.ts";
import type { CommandOutcome, HostCommand, ViewEntry } from "../../transport/protocol.ts";
import { loadQueueStrip } from "../queue-chunk.ts";
import { defaultOutcome, mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  Reflect.deleteProperty(document, "execCommand");
});

const QUESTION = "Also check the summoning order.";
const field = () => h!.one("composer") as HTMLTextAreaElement;

const running = (): ViewEntry => ({
  header: { ...header("running").header!, steer_joins_turn: true } as NonNullable<
    ViewEntry["header"]
  >,
});
const queued = (order: number, n: number, text: string): ViewEntry => ({
  user_message: {
    ...userMessage(order, n, text).user_message!,
    status: "queued",
    turn: uuid(n, 4),
  },
});

/** The engine's editing, as WebKit and Chromium do it: the selection replaced, then an input event. */
function engine() {
  const calls: [string, string | undefined][] = [];
  const exec = (command: string, _ui?: boolean, value?: string) => {
    calls.push([command, value]);
    const a = document.activeElement;
    if (!(a instanceof HTMLTextAreaElement)) return false;
    const insert = command === "delete" ? "" : (value ?? "");
    const next = a.value.slice(0, a.selectionStart) + insert + a.value.slice(a.selectionEnd);
    Reflect.set(HTMLTextAreaElement.prototype, "value", next, a);
    a.dispatchEvent(new Event("input", { bubbles: true }));
    return true;
  };
  Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
  return calls;
}

const accept = (c: HostCommand): CommandOutcome =>
  c.send_prompt !== undefined
    ? { prompt_accepted: { turn: uuid(9, 4), message: uuid(9, 5) } }
    : defaultOutcome(c);

test.each([
  ["Send now", "steer-send-now"],
  ["Queue", "queue"],
])(
  "after the hint's %s the focus is in the field and the emptied words can be undone",
  async (_, id) => {
    h = await mountRoom({ dispatch: accept });
    h.deliver([reset([userMessage(1, 1, "Q")], running())]);
    await h.type(QUESTION);
    const calls = engine();
    const button = h.one(id) as HTMLButtonElement;
    button.focus();
    await h.click(button);
    expect(h.sent("send_prompt")).toHaveLength(1);
    expect(field().value).toBe("");
    expect(document.activeElement).toBe(field());
    // emptied through the engine's editing, the one step Cmd/Ctrl+Z takes back, not set in place
    expect(calls).toEqual([["delete", undefined]]);
  },
);

/** The room with one queued message, and the strip's chunk loaded; its edit refused as the message had gone. */
async function lastQueued() {
  h = await mountRoom({
    dispatch: (c) => {
      if (c.edit_queued_prompt !== undefined)
        throw { code: "thread.turn_not_queued", message: "raw host text", retryable: false };
      return defaultOutcome(c);
    },
  });
  h.deliver([reset([userMessage(1, 1, "Q"), queued(5, 2, "Check the lease.")], running())]);
  await act(async () => void (await loadQueueStrip()));
  await h.settle();
  await h.click(h.one("queued-edit"));
  const edit = h.host.querySelector<HTMLTextAreaElement>(".queue-edit")!;
  await act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", "Check the lease and the deed.", edit);
    edit.dispatchEvent(new Event("input", { bubbles: true }));
  });
  return () =>
    h!.click([...h!.host.querySelectorAll("button")].find((b) => b.textContent === "Save"));
}

test("an edit refused as the last queued message went comes back into the empty field, and the working line says it had gone", async () => {
  const save = await lastQueued();
  await save();
  // the message went as the edit was on its way
  h!.deliver([
    upsert({ user_message: { ...queued(5, 2, "Check the lease.").user_message!, status: "sent" } }),
  ]);
  await h!.settle();
  expect(h!.one("queued")).toBeNull();
  expect(field().value).toBe("Check the lease and the deed.");
  expect(h!.one("error")?.textContent).toBe("That message had already gone.");
  expect(h!.host.textContent).not.toContain("raw host text");
});

test("a lost edit never overwrites the field's words: it follows them after a blank line", async () => {
  const save = await lastQueued();
  await h!.type("My own words.");
  await save();
  await act(async () => h!.store.flush());
  expect(field().value).toBe("My own words.\n\nCheck the lease and the deed.");
  expect(h!.one("error")?.textContent).toBe("That message had already gone.");
});
