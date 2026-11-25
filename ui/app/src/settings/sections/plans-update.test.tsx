// @vitest-environment happy-dom
// Plans after a newer Clauseo installed over an older one (store/agent-update.ts): a plan the lawyer has, its install
// redone (stage D's events flagged installed_before), reads "Updating {plan}" with its progress, never "Add {plan}" and
// no sign-in; paused, cancelled, offline or failed says why with Try again. And "Add {plan}"'s sign-in waits for the
// status answer after Ready (plans-ready.ts): signed in sends nothing, signed out sends start_sign_in once.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import type { AgentId, InstallProgress, SetupEvent } from "../../transport/protocol.ts";
import {
  CLAUDE,
  CODEX,
  AT,
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
import { WAIT_MS } from "./plans-ready.ts";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

beforeEach(() => localStorage.clear());
afterEach(async () => {
  await unmount();
  vi.useRealTimers();
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

test("both plans installed before and missing, nothing started: each needs its new version with its size, never Add", async () => {
  const t = await mountPlans({
    first: [
      install(CLAUDE, { missing: { download_bytes: 182_400_000, installed_before: true } }),
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, { missing: { download_bytes: 201_000_000, installed_before: true } }),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  expect(block("claude")!.textContent).toContain("Claude needs its new version, about 180 MB.");
  expect(block("chatgpt")!.textContent).toContain("ChatGPT needs its new version, about 200 MB.");
  expect(block("chatgpt")!.textContent).not.toContain("stopped");
  noAdd("claude");
  noAdd("chatgpt");
  await press(button("Download", block("chatgpt")!));
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

const starts = (t: Awaited<ReturnType<typeof addChatGpt>>) =>
  t.setups().filter((c) => "start_sign_in" in c);

/** Time passing on the fake clock (shouldAdvanceTime keeps the rig's own waits going). */
async function wait(ms: number) {
  await act(async () => {
    vi.advanceTimersByTime(ms);
  });
}

test("Ready and no status answer at all (a second Add in the same run, an older desktop): start_sign_in once after the wait, then nothing", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const t = await addChatGpt();
  await play(t, install(CODEX, READY));
  await wait(WAIT_MS - 1_000);
  expect(starts(t)).toHaveLength(0);
  await wait(1_500);
  expect(t.setups()).toEqual([
    { install_agent: { agent: CODEX } },
    { start_sign_in: { agent: CODEX } },
  ]);
  // the watch ended: a much later signed out (a failed launch's re-check) opens nothing
  await play(t, signIn(CODEX, "checking"), signIn(CODEX, { signed_out: {} }));
  await wait(WAIT_MS * 2);
  expect(starts(t)).toHaveLength(1);
});

test("a signed out heard BEFORE Ready is no answer: nothing at Ready, the wait decides, once", async () => {
  // desktop must send its status answer after Ready reaches the window; a state before it is the placeholder desktop
  // shows for a plan with no sign-in state yet, so only the wait's end can act on it
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const t = await addChatGpt();
  await play(t, signIn(CODEX, { signed_out: {} }), install(CODEX, READY));
  expect(starts(t)).toHaveLength(0);
  await wait(WAIT_MS + 500);
  expect(starts(t)).toHaveLength(1);
  await wait(WAIT_MS * 2);
  expect(starts(t)).toHaveLength(1);
});

test("a long checking: nothing while it checks, nothing when the wait runs out on it, and a late signed out sends nothing", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const t = await addChatGpt();
  await play(t, install(CODEX, READY), signIn(CODEX, "checking"));
  await wait(WAIT_MS + 500);
  expect(starts(t)).toHaveLength(0);
  await play(t, signIn(CODEX, { signed_out: {} }));
  await wait(WAIT_MS * 2);
  expect(starts(t)).toHaveLength(0);
});

test("an error answer after Ready: no start_sign_in, then or later", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const t = await addChatGpt();
  await play(
    t,
    install(CODEX, READY),
    signIn(CODEX, "checking"),
    signIn(CODEX, { failed: { code: "signin.failed", at: AT } }),
  );
  await wait(WAIT_MS * 2);
  expect(starts(t)).toHaveLength(0);
});

test("a press in the block ends the wait: the lawyer's own sign-in, and no second one at the wait's end", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const t = await addChatGpt();
  await play(t, signIn(CODEX, { signed_out: {} }), install(CODEX, READY));
  await press(block("chatgpt")!.querySelector<HTMLElement>('[data-testid="plans-find-login"]'));
  await wait(WAIT_MS * 2);
  expect(starts(t)).toHaveLength(0);
  expect(t.setups()).toContainEqual({ find_login: { agent: CODEX } });
});

test("the plan leaving Ready ends the wait", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const t = await addChatGpt();
  await play(t, install(CODEX, READY));
  await play(t, install(CODEX, { missing: { download_bytes: 165_000_000 } }));
  await wait(WAIT_MS * 2);
  expect(starts(t)).toHaveLength(0);
});

/** The settings window moved to another section and back: Plans unmounts and mounts again over the same store. */
async function reopenPlans(t: Awaited<ReturnType<typeof addChatGpt>>) {
  for (const id of ["usage", "plans"]) {
    await act(async () => {
      history.replaceState(null, "", `/settings.html#${id}`);
      globalThis.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    await play(t);
    // away at Usage, Plans is gone from the page
    if (id === "usage") expect(block("chatgpt")).toBeNull();
  }
}

test("Plans closed and opened mid-add: the wait is neither lost nor doubled", async () => {
  const t = await addChatGpt();
  await reopenPlans(t);
  expect(block("chatgpt")).not.toBeNull();
  await play(
    t,
    install(CODEX, READY),
    signIn(CODEX, "checking"),
    signIn(CODEX, { signed_out: {} }),
  );
  expect(starts(t)).toHaveLength(1);
  await reopenPlans(t);
  await play(t, signIn(CODEX, { signed_out: {} }));
  expect(starts(t)).toHaveLength(1);
});
