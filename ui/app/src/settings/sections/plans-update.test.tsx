// @vitest-environment happy-dom
// Plans after a newer Clauseo installed over an older one (store/agent-update.ts): a plan the lawyer has, its install
// redone (stage D's events flagged installed_before), reads "Updating {plan}" with its progress, never "Add {plan}" and
// no sign-in; paused, cancelled, offline or failed says why with Try again. And "Add {plan}"'s sign-in waits for the
// status answer after Ready (plans-ready.ts): signed in sends nothing, signed out sends start_sign_in once.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import type { AgentId, InstallProgress, SetupEvent } from "../../transport/protocol.ts";
import {
  CLAUDE,
  CODEX,
  READY,
  block,
  button,
  install,
  mountPlans,
  play,
  press,
  signIn,
  signed,
  unmount,
} from "./plans-rig.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

beforeEach(() => localStorage.clear());
afterEach(async () => {
  await unmount();
});

/** Stage D's install event for an added plan, flagged installed_before (not in today's bindings: agent-update.ts). */
const updating = (agent: AgentId, state: InstallProgress): SetupEvent =>
  ({ agent_install_progress: { agent, state, installed_before: true } }) as SetupEvent;
const down = (mb: number): InstallProgress => ({
  downloading: { received: mb * 1_000_000, total: 201_000_000 },
});
const LOG = ["x"];
const HOSTS = ["registry.npmjs.org"];

/** Claude signed in, ChatGPT redone after an update: its first Downloading, no sign-in state. */
const UPDATING = [install(CLAUDE, READY), signIn(CLAUDE, signed()), updating(CODEX, down(0))];

const noAdd = (plan: "claude" | "chatgpt") => {
  const b = block(plan)!;
  expect(b.querySelector('[data-testid="plans-add"]')).toBeNull();
  expect(b.querySelector('[data-testid="plans-sign-in"]')).toBeNull();
  expect(b.textContent).not.toContain("Add ");
  expect(b.textContent).not.toContain("Needs a download");
};

test("ChatGPT updating: Updating ChatGPT with its progress, never Add, no sign-in, and none sent after Ready", async () => {
  const t = await mountPlans({ first: UPDATING });
  expect(block("chatgpt")!.dataset["state"]).toBe("install");
  expect(block("chatgpt")!.textContent).toContain("Updating ChatGPT… 0 MB of 201 MB");
  noAdd("chatgpt");
  await play(t, updating(CODEX, down(50)));
  expect(block("chatgpt")!.textContent).toContain("Updating ChatGPT… 50 MB of 201 MB");
  expect(block("chatgpt")!.querySelector(".fl-bar")).not.toBeNull();
  await play(t, updating(CODEX, "installing"));
  expect(block("chatgpt")!.textContent).toContain("Updating ChatGPT…");
  noAdd("chatgpt");
  await play(
    t,
    updating(CODEX, READY),
    signIn(CODEX, "checking"),
    signIn(CODEX, signed({ plan: "ChatGPT Plus", source: "reused" })),
  );
  expect(block("chatgpt")!.dataset["state"]).toBe("signed");
  expect(t.setups()).toEqual([]);
});

test("both plans installed before and missing (a host before stage D): each update stopped, never Add", async () => {
  const t = await mountPlans({
    first: [
      install(CLAUDE, { missing: { download_bytes: 182_400_000, installed_before: true } }),
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, { missing: { download_bytes: 201_000_000, installed_before: true } }),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  expect(block("claude")!.textContent).toContain("Claude's update stopped.");
  expect(block("chatgpt")!.textContent).toContain("ChatGPT's update stopped.");
  noAdd("claude");
  noAdd("chatgpt");
  await press(button("Try again", block("chatgpt")!));
  expect(t.setups()).toEqual([{ install_agent: { agent: CODEX } }]);
});

test("ChatGPT's update cancelled: says it stopped, and Try again starts it again", async () => {
  const t = await mountPlans({ first: UPDATING });
  await play(
    t,
    updating(CODEX, { missing: { download_bytes: 201_000_000, installed_before: true } }),
  );
  expect(block("chatgpt")!.textContent).toContain("ChatGPT's update stopped.");
  noAdd("chatgpt");
  await press(button("Try again", block("chatgpt")!));
  expect(t.setups()).toEqual([{ install_agent: { agent: CODEX } }]);
});

test("ChatGPT's update paused: says so, and Try again goes on with it", async () => {
  const t = await mountPlans({ first: UPDATING });
  await play(t, updating(CODEX, "paused"));
  expect(block("chatgpt")!.textContent).toContain("ChatGPT's update is paused.");
  noAdd("chatgpt");
  await press(button("Try again", block("chatgpt")!));
  expect(t.setups()).toEqual([{ retry_agent_install: { agent: CODEX } }]);
});

test.each([
  [
    "failed",
    { failed: { code: "agent.install_failed", hosts: HOSTS, log: LOG } },
    "ChatGPT's update couldn't be downloaded.",
  ],
  [
    "offline",
    { waiting: { code: "agent.install_offline", hosts: HOSTS, log: LOG } },
    "It tries again by itself.",
  ],
  [
    "set up failed",
    { failed: { code: "agent.install_setup_failed", hosts: HOSTS, log: LOG } },
    "Details",
  ],
  [
    "disk full",
    {
      failed: {
        code: "agent.install_disk_full",
        need_mb: 400,
        free_mb: 120,
        hosts: HOSTS,
        log: LOG,
      },
    },
    "ChatGPT needs 400 MB free on this computer, and 120 MB is free.",
  ],
] as [string, InstallProgress, string][])(
  "ChatGPT's update %s: the reason, and Try again sends retry, never Add",
  async (_, state, words) => {
    const t = await mountPlans({ first: UPDATING });
    await play(t, updating(CODEX, state));
    expect(block("chatgpt")!.textContent).toContain(words);
    noAdd("chatgpt");
    await press(button("Try again", block("chatgpt")!));
    expect(t.setups()).toEqual([{ retry_agent_install: { agent: CODEX } }]);
  },
);

test("ChatGPT's update on a blocked network: the blocked words and the proxy, never Add", async () => {
  const t = await mountPlans({ first: UPDATING });
  await play(
    t,
    updating(CODEX, { failed: { code: "agent.install_blocked", hosts: HOSTS, log: LOG } }),
  );
  expect(block("chatgpt")!.textContent).toContain("Your network is blocking the download.");
  noAdd("chatgpt");
  await press(button("Use the system proxy", block("chatgpt")!));
  expect(t.setups()).toEqual([{ retry_agent_install: { agent: CODEX, use_system_proxy: true } }]);
});

const NOT_ADDED = [
  install(CLAUDE, READY),
  signIn(CLAUDE, signed()),
  install(CODEX, { missing: { download_bytes: 165_000_000 } }),
  signIn(CODEX, { signed_out: {} }),
];

async function addChatGpt() {
  const t = await mountPlans({ first: NOT_ADDED });
  await press(block("chatgpt")!.querySelector<HTMLElement>('[data-testid="plans-add"]'));
  expect(t.setups()).toEqual([{ install_agent: { agent: CODEX } }]);
  await play(t, install(CODEX, { downloading: { received: 1, total: 165_000_000 } }));
  return t;
}

test("Add ChatGPT, then Ready, then the status check finds the login: no start_sign_in", async () => {
  const t = await addChatGpt();
  await play(t, install(CODEX, READY));
  // nothing yet at Ready: the status check desktop asked runs
  expect(t.setups()).toEqual([{ install_agent: { agent: CODEX } }]);
  await play(t, signIn(CODEX, "checking"));
  await play(t, signIn(CODEX, signed({ plan: "ChatGPT Plus", source: "reused" })));
  expect(t.setups()).toEqual([{ install_agent: { agent: CODEX } }]);
  expect(block("chatgpt")!.dataset["state"]).toBe("signed");
  // a later sign-out is the lawyer's: no sign-in starts by itself
  await play(t, signIn(CODEX, { signed_out: {} }));
  expect(t.setups().filter((c) => "start_sign_in" in c)).toHaveLength(0);
});

test("Add ChatGPT, then Ready, then the status check says signed out: start_sign_in once", async () => {
  const t = await addChatGpt();
  await play(t, install(CODEX, READY));
  expect(t.setups().filter((c) => "start_sign_in" in c)).toHaveLength(0);
  await play(t, signIn(CODEX, "checking"), signIn(CODEX, { signed_out: {} }));
  expect(t.setups()).toEqual([
    { install_agent: { agent: CODEX } },
    { start_sign_in: { agent: CODEX } },
  ]);
  await play(t, signIn(CODEX, { signed_out: {} }), install(CODEX, READY));
  expect(t.setups().filter((c) => "start_sign_in" in c)).toHaveLength(1);
});
