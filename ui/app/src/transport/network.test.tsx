// @vitest-environment happy-dom
// The network signal (network.ts): the first mount sends what navigator.onLine reads, online too; after that
// set_network goes to the host on a change only, ahead of a send_prompt when navigator.onLine changed with no event;
// a remount on the same transport sends nothing; unmount stops listening.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { FakeTransport } from "./fake-transport.ts";
import { useNetworkSignal } from "./network.ts";
import type { HostTransport } from "./host-transport.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let onLine = true;
beforeEach(() => {
  onLine = true;
  vi.spyOn(navigator, "onLine", "get").mockImplementation(() => onLine);
});
afterEach(() => vi.restoreAllMocks());

function Probe({ transport }: { transport: HostTransport }) {
  useNetworkSignal(transport);
  return null;
}

function mount(transport: HostTransport, strict = false): () => void {
  const root = createRoot(document.createElement("div"));
  const probe = <Probe transport={transport} />;
  act(() => root.render(strict ? <StrictMode>{probe}</StrictMode> : probe));
  return () => act(() => root.unmount());
}

const kinds = (t: FakeTransport) =>
  t.only("dispatch").map((c) => {
    const body = c.command.command;
    return typeof body === "object" && body.set_network !== undefined
      ? { set_network: body.set_network }
      : Object.keys(body as object)[0];
  });

const fire = (type: "offline" | "online") => {
  onLine = type === "online";
  window.dispatchEvent(new Event(type));
};

const ON = { set_network: { online: true } };
const OFF = { set_network: { online: false } };
const prompt = (t: FakeTransport, id: string) =>
  t.dispatch({ command_id: id, command: { send_prompt: { thread: "t-1", text: "hi" } } as never });

test("mounting online sends online: true once", () => {
  const t = new FakeTransport();
  const unmount = mount(t);
  expect(kinds(t)).toEqual([ON]);
  unmount();
});

test("mounting offline sends online: false once", () => {
  onLine = false;
  const t = new FakeTransport();
  const unmount = mount(t);
  expect(kinds(t)).toEqual([OFF]);
  unmount();
});

test("a StrictMode remount, or a later mount, on the same transport sends nothing extra", () => {
  const t = new FakeTransport();
  mount(t, true)();
  expect(kinds(t)).toEqual([ON]);
  const unmount = mount(t);
  expect(kinds(t)).toEqual([ON]);
  unmount();
});

test("after mount a change sends set_network once; a repeat sends nothing", () => {
  const t = new FakeTransport();
  const unmount = mount(t);
  fire("offline");
  expect(kinds(t)).toEqual([ON, OFF]);
  fire("offline");
  expect(kinds(t)).toEqual([ON, OFF]);
  fire("online");
  expect(kinds(t)).toEqual([ON, OFF, ON]);
  fire("online");
  expect(kinds(t)).toEqual([ON, OFF, ON]);
  unmount();
});

test("a turn start reads navigator.onLine and sends set_network ahead of the prompt", async () => {
  const t = new FakeTransport();
  const unmount = mount(t);
  onLine = false; // no event fired
  await prompt(t, "p");
  expect(kinds(t)).toEqual([ON, OFF, "send_prompt"]);
  // a second turn while still offline sends nothing more
  await prompt(t, "q");
  expect(kinds(t)).toEqual([ON, OFF, "send_prompt", "send_prompt"]);
  unmount();
});

test("unmount removes the listeners", async () => {
  const t = new FakeTransport();
  mount(t)();
  fire("offline");
  await prompt(t, "p");
  expect(kinds(t)).toEqual([ON, "send_prompt"]);
});

test("a failed send is ignored", async () => {
  const t = new FakeTransport();
  vi.spyOn(t, "dispatch").mockRejectedValue({ code: "internal", message: "x", retryable: false });
  const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
  const unmount = mount(t);
  fire("offline");
  await vi.waitFor(() => expect(debug).toHaveBeenCalledTimes(2));
  unmount();
});
