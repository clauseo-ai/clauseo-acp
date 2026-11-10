// The setup store's queue and its chosen plan, without a room: one download at a time (a new install pauses another's
// and drops its sign-in, 0033), so a plan's start_sign_in waits while the other is busy and goes once it is not; and
// before any press the chosen plan follows the one plan signed in.
import { expect, test } from "vitest";
import type { HostTransport } from "../transport/host-transport.ts";
import type { SetupCommand, SetupEvent } from "../transport/protocol.ts";
import { busy, CLAUDE, CODEX, createSetupStore, type SetupSource } from "./setup.ts";

/** A store over a stub host: what it sent, and a way to say what the host says. */
function rig(refuse = false) {
  const sent: SetupCommand[] = [];
  let hear: (event: SetupEvent) => void = () => {};
  const transport = {
    onSetup: (listener: (event: SetupEvent) => void) => {
      hear = listener;
      return () => {};
    },
    setup: async (command: SetupCommand) => {
      sent.push(command);
      if (refuse) throw { code: "command.conflict", message: "no", retryable: true };
    },
    countUi: async () => {},
    query: async () => {
      throw new Error("no settings");
    },
    dispatch: async () => "done",
  } as unknown as HostTransport & SetupSource;
  const store = createSetupStore(transport);
  store.start();
  return { store, sent, say: (event: SetupEvent) => hear(event) };
}
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

test("busy: a download, a set-up, an offline wait and a sign-in under way are; a failure, a pause, a login found and signed in are not", () => {
  expect(busy({ install: { downloading: { received: 1, total: 2 } } })).toBe(true);
  expect(busy({ install: "installing" })).toBe(true);
  expect(
    busy({ install: { waiting: { code: "agent.install_offline", hosts: [], log: [] } } }),
  ).toBe(true);
  expect(busy({ signIn: "queued" })).toBe(true);
  expect(busy({ signIn: { waiting: { since: 0, nudge: false } } })).toBe(true);
  expect(busy({ install: { failed: { code: "agent.install_failed", hosts: [], log: [] } } })).toBe(
    false,
  );
  expect(busy({ install: "paused" })).toBe(false);
  expect(busy({ signIn: { login_found: { from: "codex" } } })).toBe(false);
  expect(busy({ signIn: { signed_in: { method: "plan", source: "own", at: 0 } } })).toBe(false);
  expect(busy(undefined)).toBe(false);
});

test("a second plan pressed at once waits, even before the host has said anything of the first", async () => {
  const { store, sent } = rig();
  store.signInTo(CLAUDE);
  store.signInTo(CODEX);
  await flush();
  expect(sent).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(store.state().queued).toBe(CODEX);
  expect(store.state().chosen).toBe(CODEX);
  expect(store.state().picked).toBe(true);
});

test("a refused first sign-in releases the waiting plan", async () => {
  const { store, sent } = rig(true);
  store.signInTo(CLAUDE);
  store.signInTo(CODEX);
  await flush();
  await flush();
  expect(sent).toEqual([{ start_sign_in: { agent: CLAUDE } }, { start_sign_in: { agent: CODEX } }]);
  expect(store.state().queued).toBeNull();
});

test("a plan already signed in, or whose sign-in is under way, only becomes the chosen one", async () => {
  const { store, sent, say } = rig();
  say({
    sign_in_changed: {
      agent: CODEX,
      state: { signed_in: { method: "plan", source: "own", at: 0 } },
    },
  });
  say({ sign_in_changed: { agent: CLAUDE, state: { waiting: { since: 0, nudge: false } } } });
  store.signInTo(CODEX);
  store.signInTo(CLAUDE);
  await flush();
  expect(sent).toEqual([]);
  expect(store.state().chosen).toBe(CLAUDE);
});

test("before any press the chosen plan follows the one plan signed in; with both or none it stays; a press fixes it", () => {
  const { store, say } = rig();
  expect(store.state().chosen).toBe(CLAUDE);
  say({ sign_in_changed: { agent: CLAUDE, state: { signed_out: {} } } });
  say({
    sign_in_changed: {
      agent: CODEX,
      state: { signed_in: { method: "plan", source: "own", at: 0 } },
    },
  });
  expect(store.state().chosen).toBe(CODEX);
  say({
    sign_in_changed: {
      agent: CLAUDE,
      state: { signed_in: { method: "plan", source: "own", at: 0 } },
    },
  });
  expect(store.state().chosen).toBe(CODEX);
  say({ sign_in_changed: { agent: CODEX, state: { signed_out: {} } } });
  expect(store.state().chosen).toBe(CLAUDE);
  store.signInTo(CODEX);
  say({ sign_in_changed: { agent: CODEX, state: { signed_out: {} } } });
  expect(store.state().chosen).toBe(CODEX);
});
