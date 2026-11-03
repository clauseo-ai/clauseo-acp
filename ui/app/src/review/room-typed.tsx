// A review state that types into the room's composer and then may press one control: matters/steer-claude and
// steer-chatgpt's hint (words in the field while the answer runs), and the host's refusal of the hint's Send now.
import { useEffect, useRef } from "react";
import type { FakeTransport } from "../transport/fake-transport.ts";
import { RoomOver } from "./states.tsx";

/** The room, its field given `words` once it takes them, and then, with `press`, the control with that test id
 * pressed once it shows: Send now from the hint, for a state that follows it, such as the host's refusal. */
export function RoomTyped({
  host,
  words,
  press,
}: {
  host: () => FakeTransport;
  words: string;
  press?: string;
}) {
  const box = useRef<HTMLDivElement>(null);
  // how far along, kept across Strict Mode's second run, so nothing is typed or pressed twice
  const done = useRef<"none" | "typed" | "pressed">("none");
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    const go = () => {
      if (done.current === "none") {
        const field = el.querySelector<HTMLTextAreaElement>('[data-testid="composer"]');
        if (field === null || field.disabled) return;
        done.current = "typed";
        // the prototype's setter, past React's own value tracker, then the input React hears
        Reflect.set(HTMLTextAreaElement.prototype, "value", words, field);
        field.dispatchEvent(new Event("input", { bubbles: true }));
      }
      if (done.current === "typed" && press !== undefined) {
        const target = el.querySelector<HTMLElement>(`[data-testid="${press}"]`);
        if (target === null) return;
        done.current = "pressed";
        target.click();
      }
    };
    const watch = new MutationObserver(go);
    watch.observe(el, { subtree: true, childList: true, attributes: true });
    go();
    return () => watch.disconnect();
  }, [words, press]);
  return (
    <div ref={box} className="contents">
      <RoomOver host={host} />
    </div>
  );
}
