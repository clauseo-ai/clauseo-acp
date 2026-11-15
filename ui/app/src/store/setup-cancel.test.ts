// The setup store's two cancels (installs-cancel): "Cancel {name}'s sign-in" on a plan waiting behind the other plan's
// sign-in sends cancel_sign_in for that one, and once the host says its new state the waiting plan's start_sign_in goes
// with no further press; and a plan's Cancel while it isn't installed yet sends cancel_agent_install (never
// cancel_sign_in), clearing the store's own wait, so the host's `missing` leaves it not installed.
import { expect, test } from "vitest";
import type { HostTransport } from "../transport/host-transport.ts";
import type { SetupCommand, SetupEvent } from "../transport/protocol.ts";
import { CLAUDE, CODEX, createSetupStore, type SetupSource } from "./setup.ts";

/** A store over a stub host: what it sent, and a way to say what the host says. */
function rig() {
  const sent: SetupCommand[] = [];
  let hear: (event: SetupEvent) => void = () => {};
  const transport = {
    onSetup: (listener: (event: SetupEvent) => void) => {
      hear = listener;
      return () => {};
    },
    setup: async (command: SetupCommand) => {
      sent.push(command);
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

const install = (agent: string, state: unknown): SetupEvent =>
  ({ agent_install_progress: { agent, state } }) as SetupEvent;
const signIn = (agent: string, state: unknown): SetupEvent =>
  ({ sign_in_changed: { agent, state } }) as SetupEvent;
const READY = { ready: { version: "1" } };
const DOWNLOADING = { downloading: { received: 1, total: 2 } };
const MISSING = { missing: { download_bytes: 2 } };
const BROWSER = { waiting: { since: 0, nudge: false } };

test("Cancel Claude's sign-in, from ChatGPT waiting in the store: cancel_sign_in goes for Claude, and ChatGPT's start_sign_in goes by itself once Claude's state changes", async () => {
  const { store, sent, say } = rig();
  say(install(CLAUDE, READY));
  say(signIn(CLAUDE, BROWSER));
  say(install(CODEX, READY));
  say(signIn(CODEX, { signed_out: {} }));
  store.signInTo(CODEX);
  await flush();
  expect(store.state().queued).toBe(CODEX);
  store.cancelAhead(CODEX);
  await flush();
  expect(sent).toEqual([{ cancel_sign_in: { agent: CLAUDE } }]);
  // still waiting until the host says Claude's new state
  expect(store.state().queued).toBe(CODEX);
  say(signIn(CLAUDE, { failed: { code: "signin.cancelled", at: 0 } }));
  await flush();
  expect(sent).toEqual([
    { cancel_sign_in: { agent: CLAUDE } },
    { start_sign_in: { agent: CODEX } },
  ]);
  expect(store.state().queued).toBeNull();
});

test("Cancel Claude's sign-in, with desktop holding ChatGPT's: cancel_sign_in goes for Claude alone (desktop starts ChatGPT's)", async () => {
  const { store, sent, say } = rig();
  say(install(CLAUDE, READY));
  say(signIn(CLAUDE, BROWSER));
  say(install(CODEX, READY));
  say(signIn(CODEX, "queued"));
  store.cancelAhead(CODEX);
  say(signIn(CLAUDE, { signed_out: {} }));
  await flush();
  expect(sent).toEqual([{ cancel_sign_in: { agent: CLAUDE } }]);
});

test("Cancel Claude's sign-in with no sign-in ahead sends nothing", async () => {
  const { store, sent, say } = rig();
  say(install(CLAUDE, READY));
  say(signIn(CLAUDE, { signed_out: {} }));
  store.cancelAhead(CODEX);
  await flush();
  expect(sent).toEqual([]);
});

test("Cancel while the plan downloads with its sign-in queued at desktop: cancel_agent_install, not cancel_sign_in; the host's missing leaves it not installed", async () => {
  const { store, sent, say } = rig();
  say(install(CODEX, DOWNLOADING));
  say(signIn(CODEX, "queued"));
  store.cancelSignIn(CODEX);
  await flush();
  expect(sent).toEqual([{ cancel_agent_install: { agent: CODEX } }]);
  say(install(CODEX, MISSING));
  say(signIn(CODEX, { signed_out: {} }));
  expect(store.state().agents[CODEX]?.install).toEqual(MISSING);
});

test("Cancel on the store's own wait while the plan downloads: the wait clears and cancel_agent_install goes; Claude's end starts nothing", async () => {
  const { store, sent, say } = rig();
  say(install(CLAUDE, READY));
  say(signIn(CLAUDE, BROWSER));
  say(install(CODEX, MISSING));
  say(signIn(CODEX, { signed_out: {} }));
  store.signInTo(CODEX);
  say(install(CODEX, DOWNLOADING));
  expect(store.state().queued).toBe(CODEX);
  store.cancelSignIn(CODEX);
  expect(store.state().queued).toBeNull();
  say(install(CODEX, MISSING));
  say(signIn(CLAUDE, { signed_out: {} }));
  await flush();
  expect(sent).toEqual([
    { install_agent: { agent: CODEX } },
    { cancel_agent_install: { agent: CODEX } },
  ]);
});

test("every state of an install not here yet takes cancel_agent_install: setting up, offline, paused, stopped; missing, ready and the firm's take cancel_sign_in", async () => {
  const going = [
    "installing",
    { waiting: { code: "agent.install_offline", retry_at: 0 } },
    "paused",
    { failed: { code: "agent.install_failed", hosts: [], log: [] } },
  ];
  for (const state of going) {
    const { store, sent, say } = rig();
    say(install(CODEX, state));
    say(signIn(CODEX, "queued"));
    store.cancelSignIn(CODEX);
    await flush();
    expect(sent).toEqual([{ cancel_agent_install: { agent: CODEX } }]);
  }
  for (const state of [MISSING, READY, "managed"]) {
    const { store, sent, say } = rig();
    say(install(CODEX, state));
    say(signIn(CODEX, "queued"));
    store.cancelSignIn(CODEX);
    await flush();
    expect(sent).toEqual([{ cancel_sign_in: { agent: CODEX } }]);
  }
});
