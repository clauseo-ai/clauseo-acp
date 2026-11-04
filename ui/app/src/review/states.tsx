// The states the registered strings show in: the real room (src/room/room.tsx) over a host scripted for the state,
// inside the page's view store, as App mounts it; and the room after the steps a state takes once rendered (New
// matter, a name, Change, Create), or after one control in the thread is pressed.
import { useEffect, useRef, useState } from "react";
import { Room } from "../room/room.tsx";
import type { StandIns } from "../room/versions/stand-ins.ts";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import type { FakeTransport } from "../transport/fake-transport.ts";
import type { Step } from "./hosts.ts";

/** The room over the transport `host` builds, built once for the frame's life. */
export function RoomOver({
  host,
  standIns,
}: {
  host: () => FakeTransport;
  /** Optional: what Versions can do once host changes land, passed to the room as the page wants it. */
  standIns?: StandIns;
}) {
  const [transport] = useState(host);
  const [store] = useState(() => createViewStore(transport, { batch: () => frameBatch }));
  return (
    <StoreContext value={store}>
      <Room transport={transport} standIns={standIns} />
    </StoreContext>
  );
}

/** The room, and once its composer takes words, the lawyer asking `question` with Enter: for a state that follows a
 * send, such as the host refusing it. With `send` false, `question` is only typed, the field focused first: for a
 * state the words open, such as the `/` menu. */
export function RoomAsking({
  host,
  question,
  send = true,
}: {
  host: () => FakeTransport;
  question: string;
  send?: boolean;
}) {
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
      // the focus asks for the menu's chunk, as a click in the field would
      if (!send) field.focus();
      // the prototype's setter, past React's own value tracker, then the input React hears
      Reflect.set(HTMLTextAreaElement.prototype, "value", question, field);
      field.dispatchEvent(new Event("input", { bubbles: true }));
      if (!send) return;
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
  }, [question, send]);
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

/** The room inside its view store, and once the element `focus` (a CSS selector) shows, that element focused as the
 * keyboard focuses it (a key first, so its focus is the keyboard's), for a state that shows a hint on keyboard focus,
 * such as a tile's tooltip. */
export function RoomFocusing({ host, focus }: { host: () => FakeTransport; focus: string }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    let focused = false;
    const tap = () => {
      const target = document.querySelector<HTMLElement>(focus);
      if (focused || target === null) return;
      focused = true;
      watch.disconnect();
      target.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
      target.focus();
    };
    const watch = new MutationObserver(tap);
    watch.observe(el.ownerDocument.body, { subtree: true, childList: true });
    tap();
    return () => watch.disconnect();
  }, [focus]);
  return (
    <div ref={box} className="contents">
      <RoomOver host={host} />
    </div>
  );
}

/** The room inside its view store, and the controls `clicks` names (CSS selectors, in order), each pressed once it
 * shows and isn't waiting: for a state some presses in, such as Versions open, a version chosen and its Restore. The
 * selectors read the whole document, as a dialog stands outside the room. */
export function RoomClicking({
  host,
  clicks,
  standIns,
}: {
  host: () => FakeTransport;
  clicks: readonly string[];
  standIns?: StandIns;
}) {
  // how far along, kept across Strict Mode's second run, so no control is pressed twice
  const done = useRef(0);
  useEffect(() => {
    const tap = () => {
      const selector = clicks[done.current];
      if (selector === undefined) return;
      const target = document.querySelector<HTMLElement>(selector);
      if (target === null || target.getAttribute("aria-disabled") === "true") return;
      done.current += 1;
      target.click();
    };
    // a tick as well as each change: a control can show before what it acts on has arrived
    const tick = setInterval(tap, 100);
    const watch = new MutationObserver(tap);
    watch.observe(document.body, { subtree: true, childList: true });
    tap();
    return () => {
      clearInterval(tick);
      watch.disconnect();
    };
  }, [clicks]);
  return <RoomOver host={host} {...(standIns === undefined ? {} : { standIns })} />;
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

/** The room over `host`, Find typed into with `words`, then its first conversation found pressed once it shows: Find
 * answers after its debounce, later than RoomAfter's next step would press. */
export function RoomFound({ host, words }: { host: () => FakeTransport; words: string }) {
  useEffect(() => {
    const poll = setInterval(() => {
      const row = document.querySelector<HTMLElement>('[data-testid="find-conversation"]');
      if (row === null) return;
      clearInterval(poll);
      row.click();
    }, 50);
    return () => clearInterval(poll);
  }, []);
  const [steps] = useState(() => [{ find: words }]);
  return <RoomAfter host={host} steps={steps} />;
}
