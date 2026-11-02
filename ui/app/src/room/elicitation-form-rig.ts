// Shared harness for the elicitation form tests (elicitation-form.test.tsx and elicitation-form-a11y.test.tsx): the room
// over FakeTransport with one question's entry, and the helpers that fill, key and read the form. Not shipped: only
// tests import it, and its afterEach runs in each file that imports it.
import { act } from "react";
import { afterEach } from "vitest";
import { header, reset, uuid } from "../testing/stream-script.ts";
import type { HostCommand, HostError, ViewEntry } from "../transport/protocol.ts";
import { askEntry, type AskForm } from "./elicitation-fixtures.ts";
import { defaultOutcome, mountRoom, type Harness } from "./thread/harness.ts";

export const ID = uuid(1, 9);
const hostError = (code: string): HostError =>
  ({ code, message: "refused", retryable: false }) as HostError;

/** The room, its turn waiting on the lawyer at these entries; answer_elicitation refused with `refuse` when given. */
export async function room(entries: ViewEntry[], refuse?: string, blur = false): Promise<Harness> {
  const harness = await mountRoom({
    dispatch: (command: HostCommand) => {
      if (refuse !== undefined && command.answer_elicitation !== undefined) throw hostError(refuse);
      return defaultOutcome(command);
    },
  });
  if (blur) await act(async () => (document.activeElement as HTMLElement | null)?.blur());
  harness.deliver([reset(entries, header("needs_you"))]);
  await harness.settle();
  return harness;
}
export const ask = (form: AskForm, refuse?: string) => room([askEntry(3, ID, form)], refuse);

/** The helpers over the test's own harness: `get` reads the harness the test holds (a test may mount again), `set`
 * clears it at teardown. The afterEach here runs in each file that calls rig. */
export function rig(get: () => Harness | null, set: (next: Harness | null) => void) {
  afterEach(async () => {
    await get()?.unmount();
    set(null);
    document.body.replaceChildren();
  });
  const el = <E extends HTMLElement = HTMLInputElement>(id: string): E => {
    const found = get()!.one(id);
    if (found === null) throw new Error(`no ${id}`);
    return found as E;
  };
  const answered = () => get()!.sent("answer_elicitation");
  async function fill(id: string, text: string) {
    const node = el<HTMLInputElement | HTMLTextAreaElement>(id);
    await act(async () => {
      const proto = node instanceof HTMLTextAreaElement ? HTMLTextAreaElement : HTMLInputElement;
      Reflect.set(proto.prototype, "value", text, node);
      node.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }
  async function keyOn(node: Element, init: KeyboardEventInit): Promise<boolean> {
    const e = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
    await act(async () => void node.dispatchEvent(e));
    await get()!.settle();
    return e.defaultPrevented;
  }
  const held = (id: string) => el<HTMLElement>(id).getAttribute("aria-disabled") === "true";
  /** Puts words in a number field the input can't read: the browser reports "" with validity.badInput. */
  async function fillBad(id: string) {
    const node = el(id);
    Object.defineProperty(node, "validity", {
      configurable: true,
      get: () => ({ badInput: true }),
    });
    await fill(id, "");
  }
  return { el, answered, fill, fillBad, keyOn, held };
}
