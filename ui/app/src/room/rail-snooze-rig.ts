// The rig for U16b's rail tests (rail-snooze.test.tsx, rail-done.test.tsx): rail-needs-rig.ts's matters (A open, B
// waiting on a permission, C with an answer ready, D pinned with a question), each matter's threads as the test gives
// them, the host answering every command (or refusing as the test says), and the room mounted over FakeTransport.
import { act, createElement } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { createRoot, type Root } from "react-dom/client";
import type { CommandEnvelope, CommandOutcome } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { fakeNext } from "./fake-next.ts";
import type { MatterInfo, ThreadInfo } from "./host-next.ts";
import { LISTS, MATTERS, settle } from "./rail-needs-rig.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

export { A, B, C, D, NOW, settle, thread, T_A, T_ASK } from "./rail-needs-rig.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

/** The room over a host with these threads (rail-needs-rig's where left out); `dispatch` answers each command. */
export async function mount({
  lists = LISTS,
  matters = MATTERS,
  dispatch,
  theme = "light",
}: {
  theme?: "light" | "dark";
  lists?: ReadonlyMap<string, readonly ThreadInfo[]>;
  matters?: () => MatterInfo[];
  dispatch?: (command: CommandEnvelope) => CommandOutcome | Promise<CommandOutcome>;
} = {}) {
  const transport = new FakeTransport({
    next: fakeNext({ matters, threads: (m) => lists.get(m) ?? [], frames: true }),
    ...(dispatch === undefined ? {} : { dispatch }),
  });
  connectRoom(transport, { language: "en", theme });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(createElement(Room, { transport })));
  await settle();
  // the shelf comes with the later chunk
  await act(async () => settled());
  await settle();
  return transport;
}

export async function unmount() {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
}

export const rail = () => host?.querySelector<HTMLElement>("nav.room-rail") ?? null;
export const needs = () => rail()?.querySelector('[data-testid="rail-needs-you"]') ?? null;
export const names = (el: Element | null | undefined) =>
  [...(el?.querySelectorAll(".room-matter-name") ?? [])].map((n) => n.textContent);
export const conversations = () => [
  ...(rail()?.querySelectorAll<HTMLElement>('[data-testid="conversation"]') ?? []),
];
export const titles = () =>
  conversations().map((c) => c.querySelector(".room-conv-title")?.textContent);
export const line = () => rail()?.querySelector<HTMLElement>('[data-testid="rail-line"]') ?? null;
export const menuItems = () => [
  ...document.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemcheckbox"]'),
];
export const item = (text: string) => menuItems().find((i) => i.textContent?.includes(text));

/** The commands sent of one kind, in order. */
export const sent = <K extends string>(transport: FakeTransport, kind: K) =>
  transport
    .only("dispatch")
    .map((c) => (c.command.command as Record<string, unknown>)[kind])
    .filter((c) => c !== undefined);

/** A press, its answers and the chunk it loads settled. */
export async function press(el: HTMLElement | null | undefined) {
  if (el == null) throw new Error("nothing to press");
  await act(async () => el.click());
  await act(async () => settled());
  await settle();
}

/** The pointer comes over the rail, or leaves it. */
export async function hover(over: boolean) {
  await act(async () =>
    rail()?.dispatchEvent(new PointerEvent(over ? "pointerover" : "pointerout", { bubbles: true })),
  );
  await settle();
}

/** The pointer comes to a conversation's row: its two buttons mount. */
export async function arm(title: string) {
  const row = conversations().find(
    (c) => c.querySelector(".room-conv-title")?.textContent === title,
  );
  await act(async () =>
    row?.closest("li")?.dispatchEvent(new PointerEvent("pointerover", { bubbles: true })),
  );
  await settle();
  return row?.closest("li") ?? null;
}

/** The window comes back to the front. */
export async function focusWindow() {
  await act(async () => window.dispatchEvent(new Event("focus")));
  await settle();
}
