// The API key's path through the room, from the Windows key test (keypath-proof-vm/results.md), over a FakeTransport
// with the setup wire: a refused key (the turn failed with agent.auth_required while the plan runs on a key) says the
// key wasn't accepted in the thread and never "Sign in first", Codex's own 401 text never shows, and the composer's line
// names the key by its maker; a key saved while the plan isn't installed starts its download, and the room holds the
// question until it is ready rather than sending it to a refusal; the key is offered while the plan isn't set up.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import {
  batch,
  event,
  header,
  reset,
  step,
  THREAD,
  userMessage,
} from "../../testing/stream-script.ts";
import { FakeTransport, type FakeHost } from "../../transport/fake-transport.ts";
import type { AgentId, SetupEvent, SignIn, ViewEntry } from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import { Room } from "../room.tsx";
import { stubFonts } from "../thread/testing.ts";
import { connectRoom } from "../start.ts";
import {
  A,
  AT,
  byId,
  CLAUDE,
  CODEX,
  info,
  install,
  mountFirstRun,
  play,
  press,
  READY,
  setups,
  settle,
  signIn,
  text,
  typeInto,
} from "./rig.tsx";

stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const KEY: SignIn = { signed_in: { method: "api_key", source: "own", at: AT } };
const ASKED = userMessage(2, 1, "What does note.txt say?");
const FAILED = step(4, { stop: "failed", failure: "agent.auth_required", ts: 4000 });
/** Codex's own warning after OpenAI refused the key, as codex-acp sends it in a `notice` (log-excerpts.txt's line, the
 * cf-ray and request id made up). */
const CODEX_401: ViewEntry = {
  notice: {
    order: 3,
    event: "00000000-0000-4000-8000-000000000301",
    level: "warning",
    text: "Falling back from WebSockets to HTTPS transport. unexpected status 401 Unauthorized: Incorrect API key provided: sk-proj-****************alid. You can find your API key at https://platform.openai.com/account/api-keys., url: wss://api.openai.com/v1/responses, cf-ray: 9f00000000000000-BOM, request id: req_00000000000000000000000000000000, auth error: 401, auth error code: invalid_api_key",
    ts: 3500,
  },
};

/** Matter A open on its one thread, run by `agent` signed in as `state` (a key unless said): its question held after the
 * turn failed with agent.auth_required, the thread's entries as given. */
async function refused(agent: AgentId, entries: ViewEntry[], state: SignIn = KEY) {
  const fake: FakeHost = {
    platform: "windows",
    query: async (query) => {
      if (query === "list_matters") return { matters: [A] };
      if (query === "get_settings") throw new Error("not scripted");
      if (typeof query === "object" && "list_threads" in query)
        return { threads: [{ thread: THREAD, matter: A.matter, agent, status: "idle" }] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    setup: {
      first: (): SetupEvent[] => [
        info(),
        install(CLAUDE, READY),
        signIn(CLAUDE, agent === CLAUDE ? state : { signed_out: {} }),
        install(CODEX, READY),
        signIn(CODEX, agent === CODEX ? state : { signed_out: {} }),
      ],
    },
    next: fakeNext({
      matters: () => [],
      agents: () => [
        { id: CLAUDE, kind: "claude" as const },
        { id: CODEX, kind: "codex" as const },
      ],
      parts: ["agents"],
    }),
    subscribe: (stream) =>
      queueMicrotask(() => {
        const head = header("idle");
        head.header!.queue_held = true;
        head.header!.agent = agent;
        transport.deliver(event(batch(1, [reset(entries, head)], undefined, stream)));
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

const stopped = () => byId("turn-stopped")?.textContent ?? "";
const plan = () => document.querySelector(".room-status-plan")?.textContent ?? "";

test("defect 3: ChatGPT's refused key: the thread says ChatGPT didn't accept the API key, never Sign in, and none of Codex's 401 text shows", async () => {
  await refused(CODEX, [ASKED, CODEX_401, FAILED]);
  const page = host?.textContent ?? "";
  expect(stopped()).toContain("ChatGPT didn't accept the API key. Your question is kept.");
  expect(page).not.toContain("Sign in with ChatGPT");
  expect(page).not.toContain("in Codex first");
  // Codex's own words: the request id, the key's masked ending, the address, the status
  for (const raw of ["req_", "sk-proj", "api.openai.com", "401", "invalid_api_key", "Falling back"])
    expect(page).not.toContain(raw);
  // the room's own card under the field says it too, with where to change it
  expect(byId("signin-expired")?.textContent).toContain(
    "ChatGPT didn't accept the API key. Your question is kept.",
  );
});

test("defect 3: Claude's refused key: the thread says Claude didn't accept the API key, never Sign in with Claude first", async () => {
  await refused(CLAUDE, [ASKED, FAILED]);
  expect(stopped()).toContain("Claude didn't accept the API key. Your question is kept.");
  expect(host?.textContent).not.toContain("Sign in with Claude first");
});

test("defect 5: with a key in use the composer's line names the key by its maker: Anthropic for Claude, OpenAI for ChatGPT", async () => {
  await refused(CLAUDE, [ASKED, FAILED]);
  expect(plan()).toBe("Uses your Anthropic API key");
  expect(host?.textContent).not.toContain("Uses your Claude plan");
  await act(async () => root?.unmount());
  host?.remove();
  await refused(CODEX, [ASKED, FAILED]);
  expect(plan()).toBe("Uses your OpenAI API key");
  expect(host?.textContent).not.toContain("Uses your ChatGPT plan");
});

test("a plan login keeps its line and its Sign in words: nothing changes without a key", async () => {
  await refused(CODEX, [ASKED, FAILED], { signed_in: { method: "plan", source: "own", at: AT } });
  expect(plan()).toBe("Uses your ChatGPT plan");
  expect(stopped()).toContain("Sign in with ChatGPT in Codex first. Your question is kept.");
});

test("defect 1: a key saved while Claude isn't installed starts its download, and the room holds the question until it is ready", async () => {
  const t = await mountFirstRun({
    first: [
      info(true),
      install(CLAUDE, { missing: { download_bytes: 160_000_000 } }),
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, { missing: { download_bytes: 139_000_000 } }),
    ],
  });
  await press("Use an API key instead");
  const key = byId<HTMLInputElement>("signin-key");
  await typeInto(key as HTMLInputElement, "sk-ant-api03-clauseo-test-invalid");
  await press("Use this key");
  expect(setups(t)).toEqual([
    { set_agent_key: { agent: CLAUDE, key: "sk-ant-api03-clauseo-test-invalid" } },
    { install_agent: { agent: CLAUDE } },
  ]);
  // the host says the key is in use and the download runs: the gate stays in the field's row, its act busy, so nothing
  // is sent to "Clauseo couldn't take that request as it was."
  await play(t, signIn(CLAUDE, KEY), install(CLAUDE, "installing"));
  expect(byId("signin")).not.toBeNull();
  expect(text()).toContain("Claude isn't set up on this computer.");
  expect(text()).not.toContain("Use an API key instead");
  expect(text()).not.toContain("Sign in with Claude");
  const act1 = byId("signin")?.querySelector<HTMLButtonElement>("button.room-field-ask");
  expect(act1?.disabled).toBe(true);
  // ready: the gate leaves the field, and the question can go
  await play(t, install(CLAUDE, READY));
  expect(byId("signin")).toBeNull();
});

test("defect 1: a key saved earlier on a plan never installed: the room's one act installs it, never a sign-in", async () => {
  const t = await mountFirstRun({
    first: [
      info(true),
      install(CLAUDE, { missing: { download_bytes: 160_000_000 } }),
      signIn(CLAUDE, KEY),
    ],
  });
  expect(text()).toContain("Claude isn't set up on this computer.");
  await press("Set up Claude");
  expect(setups(t)).toEqual([{ install_agent: { agent: CLAUDE } }]);
});

test("defect 2: ChatGPT not set up, conversation fixed to it: its set-up line offers the key beside Set up ChatGPT", async () => {
  const t = await mountFirstRun({
    first: [
      info(true),
      install(CLAUDE, READY),
      signIn(CLAUDE, { signed_in: { method: "plan", source: "own", at: AT } }),
      install(CODEX, { missing: { download_bytes: 139_000_000 } }),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  // the composer's "Plan to ask with" switch, at ChatGPT
  await press("ChatGPT");
  expect(text()).toContain("ChatGPT isn't set up on this computer.");
  await press("Use an API key instead");
  const key = byId<HTMLInputElement>("signin-key");
  expect(key?.getAttribute("aria-label")).toBe("OpenAI API key");
  await typeInto(key as HTMLInputElement, "sk-proj-clauseo-test-invalid");
  await press("Use this key");
  expect(setups(t)).toEqual([
    { set_agent_key: { agent: CODEX, key: "sk-proj-clauseo-test-invalid" } },
    { install_agent: { agent: CODEX } },
  ]);
});
