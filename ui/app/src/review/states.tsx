// The states the registered strings show in: the real room (src/room/room.tsx) over a host scripted for the state,
// inside the page's view store, as App mounts it; and the room after the steps a state takes once rendered (New
// matter, a name, Change, Create), or after one control in the thread is pressed.
import { useEffect, useRef, useState } from "react";
import { Room } from "../room/room.tsx";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import type { FakeTransport } from "../transport/fake-transport.ts";
import type { Step } from "./hosts.ts";

/** The room over the transport `host` builds, built once for the frame's life. */
export function RoomOver({ host }: { host: () => FakeTransport }) {
  const [transport] = useState(host);
  const [store] = useState(() => createViewStore(transport, { batch: () => frameBatch }));
  return (
    <StoreContext value={store}>
      <Room transport={transport} />
    </StoreContext>
  );
}

/** The room, and once its composer takes words, the lawyer asking `question` with Enter: for a state that follows a
 * send, such as the host refusing it. */
export function RoomAsking({ host, question }: { host: () => FakeTransport; question: string }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    let asked = false;
    const ask = () => {
      const field = el.querySelector<HTMLTextAreaElement>('[data-testid="composer"]');
      if (asked || field === null || field.disabled) return;
      asked = true;
      watch.disconnect();
      // the prototype's setter, past React's own value tracker, then the input React hears
      Reflect.set(HTMLTextAreaElement.prototype, "value", question, field);
      field.dispatchEvent(new Event("input", { bubbles: true }));
      queueMicrotask(() =>
        field.dispatchEvent(
          new KeyboardEvent("keydown", {
            key: "Enter",
            keyCode: 13,
            bubbles: true,
            cancelable: true,
          }),
        ),
      );
    };
    const watch = new MutationObserver(ask);
    watch.observe(el, { subtree: true, childList: true, attributes: true });
    ask();
    return () => watch.disconnect();
  }, [question]);
  return (
    <div ref={box} className="contents">
      <RoomOver host={host} />
    </div>
  );
}

/** The room inside its view store, and once a control with the test id `press` shows, that control pressed once: for
 * a state that follows a click in the thread, such as the OS asking to confirm an answer. */
export function RoomPressing({ host, press }: { host: () => FakeTransport; press: string }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    let pressed = false;
    const tap = () => {
      const target = el.querySelector<HTMLElement>(`[data-testid="${press}"]`);
      if (pressed || target === null) return;
      pressed = true;
      watch.disconnect();
      target.click();
    };
    const watch = new MutationObserver(tap);
    watch.observe(el, { subtree: true, childList: true });
    tap();
    return () => watch.disconnect();
  }, [press]);
  return (
    <div ref={box} className="contents">
      <RoomOver host={host} />
    </div>
  );
}

/** The room over `host`, then `steps` taken in order as the lawyer would, each once the last has settled. */
export function RoomAfter({ host, steps }: { host: () => FakeTransport; steps: readonly Step[] }) {
  const [transport] = useState(host);
  const taken = useRef(false);
  useEffect(() => {
    // once, though Strict Mode runs the effect twice
    if (taken.current) return;
    taken.current = true;
    const settle = () => new Promise((resolve) => setTimeout(resolve, 50));
    void (async () => {
      await settle();
      for (const step of steps) {
        if ("press" in step) {
          document.querySelector<HTMLElement>(`[data-testid="${step.press}"]`)?.click();
        } else if ("find" in step) {
          const field = document.querySelector<HTMLInputElement>('[data-testid="find"]');
          if (field !== null) {
            field.focus();
            Reflect.set(HTMLInputElement.prototype, "value", step.find, field);
            field.dispatchEvent(new Event("input", { bubbles: true }));
          }
        } else {
          const field =
            document.querySelectorAll<HTMLInputElement>('[role="dialog"] input')[step.type.field];
          if (field !== undefined) {
            Reflect.set(HTMLInputElement.prototype, "value", step.type.text, field);
            field.dispatchEvent(new Event("input", { bubbles: true }));
            // the lawyer moves on from the field, so a line that waits for that (the next date's) shows
            field.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
          }
        }
        await settle();
      }
    })();
  }, [steps]);
  return <Room transport={transport} />;
}
