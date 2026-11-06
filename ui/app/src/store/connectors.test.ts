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
