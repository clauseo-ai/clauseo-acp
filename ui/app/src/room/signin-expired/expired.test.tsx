// The expired sign-in card (UD2, limits/signin), over a FakeTransport with the setup wire: one test per row of its
// table, the words, the button and the one command it sends; no command at all once signed in again; U10's line, not
// the card, when the queue isn't held; #537's queue_held_reason preferred when the host gives it; and the window split
// (only the six sign-in commands, no Plans module in the room).
// @vitest-environment happy-dom
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, expect, test } from "vitest";
import {
  batch,
  event,
  header,
  reset,
  step,
  THREAD,
  upsert,
  userMessage,
} from "../../testing/stream-script.ts";
import { FakeTransport, type FakeHost } from "../../transport/fake-transport.ts";
import type { SetupEvent, SignIn, ViewEntry } from "../../transport/protocol.ts";
import { createSetupStore } from "../../store/setup.ts";
import { Room } from "../room.tsx";
import { stubFonts } from "../thread/testing.ts";
import { connectRoom } from "../start.ts";
import { A, byId, CLAUDE, info, install, press, READY, settle, signIn } from "../first-run/rig.tsx";
import { roomCommand } from "./setup-commands.ts";

stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const STREAM = `thread:${THREAD}`;
const ASKED = userMessage(2, 1, "What does clause nine of the lease say about notice?");
const FAILED = step(3, { stop: "failed", failure: "agent.auth_required", ts: 3000 });
const AT = 5000;

type Options = {
  signIn: SignIn;
  held?: boolean;
  reason?: string;
  entries?: ViewEntry[];
  refuse?: string;
};

/** Matter A open on its one thread, the question held after a failed turn, the Claude plan's sign-in as given. */
async function mount(o: Options) {
  const fake: FakeHost = {
    platform: "mac",
    query: async (query) => {
      if (query === "list_matters") return { matters: [A] };
      if (query === "get_settings") throw new Error("not scripted");
      if (typeof query === "object" && "list_threads" in query)
        return {
          threads: [{ thread: THREAD, matter: A.matter, agent: CLAUDE, status: "idle" }],
        };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    setup: {
      first: (): SetupEvent[] => [info(), install(CLAUDE, READY), signIn(CLAUDE, o.signIn)],
      command: () => {
        if (o.refuse !== undefined) throw { code: o.refuse, message: "refused", retryable: false };
      },
    },
    subscribe: (stream) =>
      queueMicrotask(() => {
        const head = header("idle");
        if (o.held !== false) head.header!.queue_held = true;
        if (o.reason !== undefined) Object.assign(head.header!, { queue_held_reason: o.reason });
        transport.deliver(
          event(batch(1, [reset(o.entries ?? [ASKED, FAILED], head)], undefined, stream)),
        );
      }),
  };
  const transport: FakeTransport = new FakeTransport(fake);
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  return transport;
}

const card = () => byId("signin-expired");
const words = () => card()?.textContent ?? "";
const setupKinds = (t: FakeTransport) => t.setups().map((c) => Object.keys(c)[0]);
const SIX = [
  "start_sign_in",
  "use_different_account",
  "cancel_sign_in",
  "paste_sign_in_code",
  "use_device_code",
  "open_sign_in_page",
];
const play = async (t: FakeTransport, state: SignIn) => {
  await act(async () => t.setupEvent(signIn(CLAUDE, state)));
  await settle();
};

test("expired: the words, Sign in again sending start_sign_in, and the gate leaving the field", async () => {
  // the turn fails while the lawyer watches: U10's working line would stand; the card takes its place
  const t = await mount({
    signIn: { failed: { code: "agent.auth_required", at: AT } },
    entries: [ASKED],
    held: false,
  });
  const held = header("idle");
  held.header!.queue_held = true;
  await act(async () =>
    t.deliver(event(batch(2, [upsert(FAILED), upsert(held)], undefined, STREAM))),
  );
  await settle();
  expect(card()?.dataset["state"]).toBe("expired");
  expect(words()).toContain("Your Claude sign-in has expired. Your question is kept.");
  expect(words()).toContain(
    "Sign in again opens Claude's own sign-in in your browser. Only the sign-in repeats: this matter, its papers and your settings stay as they are.",
  );
  expect(byId("signin")).toBeNull();
  // the failed step's row keeps its words in the history; the working line under the field doesn't repeat them
  const generic = errorWords("en", "agent.auth_required", { kind: "claude", name: "Claude" });
  expect(document.body.textContent?.split(generic).length).toBe(2);
  await press(byId("signin-again"));
  expect(t.setups()).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("waiting in the browser: the line, then UD1's panel; Cancel sends cancel_sign_in", async () => {
  const t = await mount({ signIn: { waiting: { since: AT, nudge: true } } });
  expect(card()?.dataset["state"]).toBe("waiting");
  expect(words()).toContain(
    "Finish signing in in your browser. Your question is asked once you're back.",
  );
  expect(words()).toContain("Didn't open?");
  await press(byId("signin-cancel"));
  expect(t.setups()).toEqual([{ cancel_sign_in: { agent: CLAUDE } }]);
});

test("signed in again: the line in each of its three forms, and the window sends nothing at all", async () => {
  const t = await mount({ signIn: { waiting: { since: AT, nudge: false } } });
  const before = t.calls.length;
  await play(t, {
    signed_in: { method: "plan", source: "own", at: AT, email: "r@m.in", plan: "Claude Max" },
  });
  expect(card()?.dataset["state"]).toBe("again");
  expect(words()).toMatch(/^Signed in again at .+ as r@m\.in, Claude Max\.$/);
  await play(t, { signed_in: { method: "plan", source: "own", at: AT, email: "r@m.in" } });
  expect(words()).toMatch(/^Signed in again at .+ as r@m\.in\.$/);
  await play(t, { signed_in: { method: "plan", source: "own", at: AT } });
  expect(words()).toMatch(/^Signed in again at [^ ]+ ?[ap]?m?\.$/);
  const after = t.calls.slice(before);
  expect(after.filter((c) => c.call === "setup")).toEqual([]);
  const kinds = after
    .filter((c) => c.call === "dispatch")
    .map((c) => Object.keys((c as { command: { command: object } }).command.command)[0]);
  for (const k of ["restart_agent", "send_prompt", "resume_queue"]) expect(kinds).not.toContain(k);
});

test("the line goes when the resumed turn starts", async () => {
  const t = await mount({ signIn: { signed_in: { method: "plan", source: "own", at: AT } } });
  expect(card()?.dataset["state"]).toBe("again");
  const resumed = header("running");
  await act(async () =>
    t.deliver(
      event(batch(2, [upsert(userMessage(4, 2, "Next")), upsert(resumed)], undefined, STREAM)),
    ),
  );
  await settle();
  expect(card()).toBeNull();
});

test("reuse stopped: UD1's line, Your question is kept, and Sign in", async () => {
  const t = await mount({ signIn: { signed_out: { found: "reused_signed_out" } } });
  expect(card()?.dataset["state"]).toBe("reuse-stopped");
  expect(words()).toContain(
    "Claude Code on this computer is signed out, so Clauseo stopped using its sign-in.",
  );
  expect(words()).toContain("Your question is kept.");
  await press(byId("signin-again"));
  expect(t.setups()).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("reused, then failed: Sign in with a different account, never Signed in again", async () => {
  const t = await mount({ signIn: { signed_out: { found: "reused_failed" } } });
  expect(card()?.dataset["state"]).toBe("reused-failed");
  expect(words()).toContain(
    "Claude Code's sign-in stopped working here, so Clauseo stopped using it.",
  );
  expect(words()).not.toContain("Signed in again");
  await press(byId("signin-again"));
  expect(t.setups()).toEqual([{ use_different_account: { agent: CLAUDE } }]);
});

test("switching: #503's lines, Your question is kept, no button", async () => {
  await mount({ signIn: { switching: { since: AT, stuck: true } } });
  expect(card()?.dataset["state"]).toBe("switching");
  expect(words()).toContain("Switching accounts.");
  expect(words()).toContain("If this goes on, quit Clauseo and open it again.");
  expect(words()).toContain("Your question is kept.");
  expect(card()?.querySelector("button")).toBeNull();
});

test("an API key refused, and the firm's account refused: their lines and no button", async () => {
  await mount({ signIn: { signed_in: { method: "api_key", source: "own", at: AT } } });
  expect(words()).toContain("Claude didn't accept the API key. Your question is kept.");
  expect(words()).toContain("To change it, open Settings, then Plans.");
  expect(card()?.querySelector("button")).toBeNull();
  await act(async () => root?.unmount());
  host?.remove();
  await mount({ signIn: { signed_in: { method: "firm", source: "own", at: AT } } });
  expect(words()).toContain(
    "Your firm's account didn't accept the question. Your question is kept.",
  );
  expect(words()).toContain("Ask your IT team.");
  expect(card()?.querySelector("button")).toBeNull();
});

test("a refused Sign in again says its code's words under the card", async () => {
  await mount({
    signIn: { failed: { code: "agent.auth_required", at: AT } },
    refuse: "signin.key_in_use",
  });
  await press(byId("signin-again"));
  expect(words()).toContain(
    errorWords("en", "signin.key_in_use", { kind: "claude", name: "Claude" }),
  );
});

test("a turn failed with agent.auth_required but the queue not held: no card", async () => {
  await mount({ signIn: { failed: { code: "agent.auth_required", at: AT } }, held: false });
  expect(card()).toBeNull();
});

test("queue_held_reason, when the host gives it, wins: sign_in shows the card, any other reason shows none", async () => {
  await mount({
    signIn: { failed: { code: "agent.auth_required", at: AT } },
    entries: [ASKED],
    reason: "sign_in",
  });
  expect(card()?.dataset["state"]).toBe("expired");
  await act(async () => root?.unmount());
  host?.remove();
  await mount({ signIn: { failed: { code: "agent.auth_required", at: AT } }, reason: "update" });
  expect(card()).toBeNull();
});

test("the window split: the room's setup commands are only the six sign-in ones", async () => {
  const t = await mount({ signIn: { waiting: { since: AT, nudge: true } } });
  await press(byId("signin-cancel"));
  await play(t, { signed_out: { found: "reused_failed" } });
  await press(byId("signin-again"));
  for (const kind of setupKinds(t)) expect(SIX).toContain(kind);
  const store = createSetupStore(t as never);
  // typed only, never sent
  void (() =>
    // @ts-expect-error: a Plans command has no typed way out of the room
    roomCommand(store, { sign_out: { agent: CLAUDE } }));
});

test("no file under room/ imports Settings' Plans section", () => {
  const roomDir = join(import.meta.dirname, "..");
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.tsx?$/.test(name)) files.push(path);
    }
  };
  walk(roomDir);
  for (const file of files.filter((f) => f !== import.meta.filename))
    expect(readFileSync(file, "utf8"), file).not.toMatch(/settings\/sections\/plans/);
});
