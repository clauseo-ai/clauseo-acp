// @vitest-environment happy-dom
// Find's results load late (U16a): the room's first render imports none of their chunk (find-results.tsx, with the
// search, its shaping and its words); the field hovered starts the import, before anything is typed.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test, vi } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import { THE_MATTER } from "./thread/harness.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// the chunk's module, counted as it is first evaluated
const evaluated = vi.hoisted(() => ({ results: 0, search: 0 }));
vi.mock("./find-results.tsx", async (actual) => {
  evaluated.results += 1;
  return await actual();
});
vi.mock("../store/search.ts", async (actual) => {
  evaluated.search += 1;
  return await actual();
});

let root: Root | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

test("the room's first render imports none of Find's results; hovering the field starts their chunk", async () => {
  const transport = new FakeTransport({
    next: fakeNext({ matters: () => [detailed(THE_MATTER)] }),
  });
  connectRoom(transport, { language: "en" });
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  await act(async () => settled());
  const field = host.querySelector<HTMLInputElement>('[data-testid="find"]');
  expect(field).not.toBeNull();
  expect(evaluated).toEqual({ results: 0, search: 0 });
  await act(async () => {
    field?.closest("label")?.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  });
  await act(async () => settled());
  expect(evaluated).toEqual({ results: 1, search: 1 });
});
