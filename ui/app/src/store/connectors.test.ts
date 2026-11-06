// The connectors store (U15): fed by connectors_changed on the app feed, with the catch-up's snapshot a new subscriber
// (or a reconnect) hears first; a repeat keeps the object, so nothing re-renders for it; it sends nothing itself.
import { expect, test } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import { NullTransport } from "../transport/host-transport.ts";
import type { ConnectorStatus, ConnectorsState } from "../transport/protocol.ts";
import { connectorsStoreOf, createConnectorsStore, NO_CONNECTORS } from "./connectors.ts";

const state = (status: ConnectorStatus, available = true): ConnectorsState => ({
  available,
  locked: false,
  connectors: [{ id: "case-law", name: "Indian Kanoon", packs: [], status }],
});

const withFeed = (first: ConnectorsState | null = null) =>
  new FakeTransport({
    next: {
      query: () => {
        throw new Error("not scripted");
      },
      first: () => (first === null ? [] : [{ connectors_changed: first }]),
    },
  });

const tick = () => new Promise<void>((resolve) => queueMicrotask(resolve));

test("before any state: nothing listed, not heard", () => {
  const store = createConnectorsStore(withFeed());
  expect(store.state()).toBe(NO_CONNECTORS);
  expect(store.heard()).toBe(false);
});

test("the catch-up's snapshot arrives first, then each connectors_changed", async () => {
  const transport = withFeed(state("not_added"));
  const store = createConnectorsStore(transport);
  let heard = 0;
  store.subscribe(() => heard++);
  await tick();
  expect(store.heard()).toBe(true);
  expect(store.state()).toEqual(state("not_added"));
  transport.appEvent({ connectors_changed: state("signing_in") });
  expect(store.state().connectors[0]?.status).toBe("signing_in");
  expect(heard).toBe(2);
});

test("the first state is heard even when it lists nothing; a repeat by value keeps the object and tells no one", () => {
  const transport = withFeed();
  const store = createConnectorsStore(transport);
  let heard = 0;
  store.subscribe(() => heard++);
  transport.appEvent({ connectors_changed: { ...NO_CONNECTORS } });
  expect(store.heard()).toBe(true);
  expect(heard).toBe(1);
  transport.appEvent({ connectors_changed: state("signed_out") });
  const kept = store.state();
  transport.appEvent({ connectors_changed: state("signed_out") });
  expect(store.state()).toBe(kept);
  expect(heard).toBe(2);
});

test("other app events change nothing", () => {
  const transport = withFeed();
  const store = createConnectorsStore(transport);
  transport.appEvent({ browser_install: { state: "done" } });
  expect(store.heard()).toBe(false);
});

test("a transport with no app feed gives a store that never hears", () => {
  const store = createConnectorsStore(new NullTransport());
  expect(store.state()).toBe(NO_CONNECTORS);
  expect(store.heard()).toBe(false);
});

test("one store per transport", () => {
  const transport = withFeed();
  expect(connectorsStoreOf(transport)).toBe(connectorsStoreOf(transport));
});

test("the store sends nothing, whatever the status", () => {
  const transport = withFeed();
  connectorsStoreOf(transport);
  const statuses: ConnectorStatus[] = [
    "not_added",
    "signing_in",
    { signed_in: { low: true, balance: { credits: 1 } } },
    { out_of_credit: { pay_url: "https://data.clauseo.chat/pay/x" } },
    "signed_out",
    "key_revoked",
    { error: { message: "no" } },
  ];
  for (const status of statuses) {
    transport.appEvent({ connectors_changed: state(status) });
    transport.appEvent({ connectors_changed: state(status, false) });
  }
  expect(transport.only("dispatch")).toEqual([]);
});

const OUT: ConnectorStatus = { out_of_credit: {} };
const hintOf = (matter: string, thread: string, turn: string) => ({
  case_law_hint: { matter, thread, turn },
});

test("hints are kept by thread and turn, dropped by matter, and cleared once the connector is added or off", () => {
  const transport = withFeed();
  const store = createConnectorsStore(transport);
  transport.appEvent({ connectors_changed: state("not_added") });
  transport.appEvent(hintOf("m1", "t1", "u1"));
  transport.appEvent(hintOf("m2", "t2", "u2"));
  expect(store.hint("t1", "u1")?.matter).toBe("m1");
  expect(store.hint("t1", "u2")).toBeUndefined();
  store.dropHints("m1");
  expect(store.hint("t1", "u1")).toBeUndefined();
  expect(store.hint("t2", "u2")?.matter).toBe("m2");
  transport.appEvent({ connectors_changed: state("signing_in") });
  expect(store.hint("t2", "u2")).toBeUndefined();
  transport.appEvent({ connectors_changed: state("not_added") });
  transport.appEvent(hintOf("m2", "t2", "u2"));
  transport.appEvent({ connectors_changed: state("not_added", false) });
  expect(store.hint("t2", "u2")).toBeUndefined();
});

test("a hidden note stays hidden while out of credit, and the set empties when credit returns", () => {
  const transport = withFeed();
  const store = createConnectorsStore(transport);
  // nothing to hide before out of credit
  store.hide("t1");
  expect(store.hidden("t1")).toBe(false);
  transport.appEvent({ connectors_changed: state(OUT) });
  store.hide("t1");
  expect(store.hidden("t1")).toBe(true);
  expect(store.hidden("t2")).toBe(false);
  transport.appEvent({ connectors_changed: state({ out_of_credit: { pay_url: "https://x" } }) });
  expect(store.hidden("t1")).toBe(true);
  transport.appEvent({ connectors_changed: state({ signed_in: { low: false } }) });
  expect(store.hidden("t1")).toBe(false);
  transport.appEvent({ connectors_changed: state(OUT) });
  expect(store.hidden("t1")).toBe(false);
});
