// @vitest-environment happy-dom
// Each matter's draft outlives its Work: the room keeps the drafts above Work's key (room.tsx), so leaving a matter and
// coming back finds the words where the lawyer left them, as a refused send's "Your question is kept." promises.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import type { MatterId } from "../../transport/protocol.ts";
import type { Drafts } from "./session.ts";
import { WorkComposer } from "./work-composer.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

const A = "01900002-0000-7000-8000-00000000000a" as MatterId;
const B = "01900002-0000-7000-8000-00000000000b" as MatterId;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
});

const field = () => host!.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
const type = async (text: string) =>
  act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", text, field());
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });

function room(drafts: Drafts) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  // as room.tsx renders Work: keyed by the conversation (here one per matter), so each composer is a new one
  const show = (matter: MatterId, onAsk: (text: string) => Promise<boolean> = async () => false) =>
    act(async () =>
      root!.render(
        <WorkComposer
          key={matter}
          drafts={drafts}
          draftKey={matter}
          commands={[]}
          disabled={false}
          busy={false}
          refused={null}
          failed={null}
          onAsk={onAsk}
          onStop={() => {}}
        />,
      ),
    );
  return show;
}

const enter = () =>
  act(async () => {
    field().dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", keyCode: 13, bubbles: true, cancelable: true }),
    );
  });

test("a matter's draft is there again after a visit to another matter; an emptied field keeps nothing", async () => {
  const drafts: Drafts = new Map();
  const show = room(drafts);
  await show(A);
  await type("What does clause nine say?");
  await show(B);
  expect(field().value).toBe("");
  await type("Another matter's words");
  await show(A);
  expect(field().value).toBe("What does clause nine say?");
  await show(B);
  expect(field().value).toBe("Another matter's words");
  await type("");
  expect(drafts.has(B)).toBe(false);
  await show(A);
  await show(B);
  expect(field().value).toBe("");
});

test("a question accepted after the lawyer left its matter doesn't come back as the draft; a refused one does", async () => {
  const drafts: Drafts = new Map();
  const show = room(drafts);
  let accept: (ok: boolean) => void = () => {};
  const held = () => new Promise<boolean>((resolve) => (accept = resolve));
  await show(A, held);
  await type("What does clause nine say?");
  await enter();
  // off to another matter while send_prompt is on its way: A's composer is gone
  await show(B);
  await act(async () => accept(true));
  await show(A, held);
  expect(field().value).toBe("");
  expect(drafts.has(A)).toBe(false);
  // refused: the question is kept, as the refusal's words promise
  await type("And clause ten?");
  await enter();
  await show(B);
  await act(async () => accept(false));
  await show(A);
  expect(field().value).toBe("And clause ten?");
});
