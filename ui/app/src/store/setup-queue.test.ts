// The setup store's queue and its chosen plan, without a room: downloads run side by side, one per plan, so a press
// starts its plan's download at once whatever the other's is doing; sign-ins go one at a time, so a plan's
// start_sign_in waits while the other's sign-in is under way and goes by itself once it ends; and before any press the
// chosen plan follows the one plan signed in.
import { expect, test } from "vitest";
import type { HostTransport } from "../transport/host-transport.ts";
import type { SetupCommand, SetupEvent } from "../transport/protocol.ts";
import { CLAUDE, CODEX, createSetupStore, signInAhead, type SetupSource } from "./setup.ts";

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

const READY = { agent: CLAUDE, state: { ready: { version: "1" } } } as const;
const install = (agent: string, state: unknown): SetupEvent =>
  ({ agent_install_progress: { agent, state } }) as SetupEvent;
const signIn = (agent: string, state: unknown): SetupEvent =>
  ({ sign_in_changed: { agent, state } }) as SetupEvent;
const DOWNLOADING = { downloading: { received: 1, total: 2 } };
const MISSING = { missing: { download_bytes: 2 } };
const BROWSER = { waiting: { since: 0, nudge: false } };
const SIGNED = { signed_in: { method: "plan", source: "own", at: 0 } };

test("signInGoing's rule: a check, a browser, a code and a switch hold the one sign-in; a sign-in queued behind its download, a login found, a failure and signed in don't", () => {
  const ahead = (state: unknown) => {
    const { store, say } = rig();
    say(signIn(CLAUDE, state));
    return signInAhead(store.state(), CODEX);
  };
  expect(ahead("checking")).toBe(CLAUDE);
  expect(ahead(BROWSER)).toBe(CLAUDE);
  expect(ahead({ device_code: { code: "AB-12", url: "u", expires_at: 0 } })).toBe(CLAUDE);
  expect(ahead({ switching: { stuck: false } })).toBe(CLAUDE);
  expect(ahead("queued")).toBeNull();
  expect(ahead({ login_found: { from: "codex" } })).toBeNull();
  expect(ahead({ failed: { code: "signin.cancelled", at: 0 } })).toBeNull();
  expect(ahead(SIGNED)).toBeNull();
});

test("downloads side by side: ChatGPT pressed while Claude downloads sends ChatGPT's start_sign_in at once, and nothing that pauses Claude", async () => {
  const { store, sent, say } = rig();
  say(install(CLAUDE, MISSING));
  say(install(CODEX, MISSING));
  store.signInTo(CLAUDE);
  say(signIn(CLAUDE, "queued"));
  say(install(CLAUDE, DOWNLOADING));
  store.signInTo(CODEX);
  await flush();
  expect(sent).toEqual([{ start_sign_in: { agent: CLAUDE } }, { start_sign_in: { agent: CODEX } }]);
  expect(store.state().queued).toBeNull();
  expect(store.state().chosen).toBe(CODEX);
  expect(store.state().picked).toBe(true);
});

test("one sign-in at a time: ChatGPT pressed while Claude signs in starts ChatGPT's download now, and its sign-in by itself once Claude's ends", async () => {
  const { store, sent, say } = rig();
  say({ agent_install_progress: READY });
  say(signIn(CLAUDE, BROWSER));
  say(install(CODEX, MISSING));
  say(signIn(CODEX, { signed_out: {} }));
  store.signInTo(CODEX);
  await flush();
  expect(sent).toEqual([{ install_agent: { agent: CODEX } }]);
  expect(store.state().queued).toBe(CODEX);
  say(install(CODEX, DOWNLOADING));
  say(install(CODEX, { ready: { version: "1" } }));
  await flush();
  // its own download done: it waits for Claude's sign-in, still sending nothing
  expect(sent).toEqual([{ install_agent: { agent: CODEX } }]);
  expect(store.state().queued).toBe(CODEX);
  say(signIn(CLAUDE, SIGNED));
  await flush();
  expect(sent).toEqual([{ install_agent: { agent: CODEX } }, { start_sign_in: { agent: CODEX } }]);
  expect(store.state().queued).toBeNull();
});

test("one sign-in at a time, both installed: a second plan pressed at once waits, even before the host has said anything of the first", async () => {
  const { store, sent, say } = rig();
  say({ agent_install_progress: READY });
  say(install(CODEX, { ready: { version: "1" } }));
  store.signInTo(CLAUDE);
  store.signInTo(CODEX);
  await flush();
  expect(sent).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(store.state().queued).toBe(CODEX);
  // Claude's sign-in cancelled: ChatGPT's goes by itself
  say(signIn(CLAUDE, BROWSER));
  say(signIn(CLAUDE, { failed: { code: "signin.cancelled", at: 0 } }));
  await flush();
  expect(sent).toEqual([{ start_sign_in: { agent: CLAUDE } }, { start_sign_in: { agent: CODEX } }]);
});

test("a refused first sign-in releases the waiting plan", async () => {
  const { store, sent, say } = rig(true);
  say({ agent_install_progress: READY });
  say(install(CODEX, { ready: { version: "1" } }));
  store.signInTo(CLAUDE);
  store.signInTo(CODEX);
  await flush();
  await flush();
  expect(sent).toEqual([{ start_sign_in: { agent: CLAUDE } }, { start_sign_in: { agent: CODEX } }]);
  expect(store.state().queued).toBeNull();
});

test("the waiting plan's Cancel stops its wait and leaves its download going: nothing is sent, and Claude's end starts nothing", async () => {
  const { store, sent, say } = rig();
  say({ agent_install_progress: READY });
  say(signIn(CLAUDE, BROWSER));
  say(install(CODEX, MISSING));
  store.signInTo(CODEX);
  say(install(CODEX, DOWNLOADING));
  store.unqueue();
  say(signIn(CLAUDE, SIGNED));
  await flush();
  expect(sent).toEqual([{ install_agent: { agent: CODEX } }]);
  expect(store.state().agents[CODEX]?.install).toEqual(DOWNLOADING);
});

test("a waiting plan whose own download fails waits no more", async () => {
  const { store, sent, say } = rig();
  say({ agent_install_progress: READY });
  say(signIn(CLAUDE, BROWSER));
  say(install(CODEX, MISSING));
  store.signInTo(CODEX);
  say(install(CODEX, { failed: { code: "agent.install_failed", hosts: [], log: [] } }));
  expect(store.state().queued).toBeNull();
  say(signIn(CLAUDE, SIGNED));
  await flush();
  expect(sent).toEqual([{ install_agent: { agent: CODEX } }]);
});

test("the gate's Sign in goes through the queue too, keeping Claude's Console sign-in", async () => {
  const { store, sent, say } = rig();
  say(install(CODEX, { ready: { version: "1" } }));
  say(signIn(CODEX, BROWSER));
  say({ agent_install_progress: READY });
  store.signIn(CLAUDE, { console: true, again: true });
  await flush();
  expect(sent).toEqual([]);
  expect(store.state().queued).toBe(CLAUDE);
  // the plain Sign in leaves the chosen plan as it was
  expect(store.state().picked).toBe(false);
  say(signIn(CODEX, SIGNED));
  await flush();
  expect(sent).toEqual([{ start_sign_in: { agent: CLAUDE, console: true } }]);
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
