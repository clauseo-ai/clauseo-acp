// The first screen offers both plans side by side and equal (the launch fix for a first run that offered Claude alone):
// a lead line, then Claude's row and ChatGPT's, each with its own size and "Sign in to {name}". Either plan alone
// installs and signs in, and the chosen plan follows the press; both pressed go one after the other, the second
// waiting with the queued line until the first is ready and signed in, or its install or sign-in ends, and no second
// install is ever sent over the first (a new install pauses another's and drops its sign-in, 0033).
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { setupStoreOf, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FakeTransport } from "../../transport/fake-transport.ts";
import type { SetupCommand } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  CODEX,
  READY,
  SIGNED_IN,
  allById,
  install,
  mountFirstRun,
  play,
  press,
  setups,
  signIn,
  text,
} from "./rig.tsx";

const FRESH = [
  install(CLAUDE, { missing: { download_bytes: 112_000_000 } }),
  signIn(CLAUDE, { signed_out: {} }),
  install(CODEX, { missing: { download_bytes: 139_000_000 } }),
  signIn(CODEX, { signed_out: {} }),
];

const rows = () => allById("plan-row") as HTMLElement[];
const row = (agent: string) => rows().find((r) => r.dataset["agent"] === agent) ?? null;
const storeOf = (t: FakeTransport) => setupStoreOf(t as unknown as HostTransport & SetupSource);
/** Whether any command would install or pause a plan's download over another's. */
const installs = (commands: SetupCommand[]) =>
  commands.filter((c) => c.install_agent !== undefined || c.retry_agent_install !== undefined);

test("a fresh computer: the lead, then Claude's row and ChatGPT's, equal, each with its size and its own Sign in", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  expect(text()).toContain("Sign in with the plan you pay for: Claude, ChatGPT or both.");
  expect(rows().map((r) => r.dataset["agent"])).toEqual([CLAUDE, CODEX]);
  expect(row(CLAUDE)?.textContent).toContain("Claude · needs a download of about 110 MB");
  expect(row(CODEX)?.textContent).toContain("ChatGPT · needs a download of about 140 MB");
  const claude = row(CLAUDE)?.querySelector("button");
  const chatgpt = row(CODEX)?.querySelector("button");
  expect(claude?.textContent).toBe("Sign in to Claude");
  expect(chatgpt?.textContent).toBe("Sign in to ChatGPT");
  // equal weight: the same row, the same button
  expect(row(CLAUDE)?.className).toBe(row(CODEX)?.className);
  expect(claude?.className).toBe(chatgpt?.className);
  // each row its own live region
  expect(allById("install-said").map((s) => s.textContent)).toEqual([
    "Claude · needs a download of about 110 MB",
    "ChatGPT · needs a download of about 140 MB",
  ]);
  // each region names its plan, as its row does (the Windows e2e finds Claude's by it)
  expect(allById("install-said").map((s) => (s as HTMLElement).dataset["agent"])).toEqual([
    CLAUDE,
    CODEX,
  ]);
  // each row starts with its plan, in the row's own tone; only offered, so no clock
  expect(rows().map((r) => r.querySelector(".working-plan")?.textContent)).toEqual([
    "Claude",
    "ChatGPT",
  ]);
  expect(rows().map((r) => r.querySelector(".working")?.getAttribute("data-lead"))).toEqual([
    "none",
    "none",
  ]);
  // the fine print names both plans, before and after a press
  expect(text()).toContain(
    "Your papers stay on this computer. Claude or ChatGPT sees the parts it reads to answer you.",
  );
  expect(setups(t)).toEqual([]);
});

test("ChatGPT alone: its Sign in installs and signs in ChatGPT, and the chosen plan follows", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in to ChatGPT");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CODEX } }]);
  expect(storeOf(t).state().chosen).toBe(CODEX);
  expect(text()).toContain(
    "Your papers stay on this computer. Claude or ChatGPT sees the parts it reads to answer you.",
  );
  await play(
    t,
    signIn(CODEX, "queued"),
    install(CODEX, { downloading: { received: 20_000_000, total: 139_000_000 } }),
  );
  expect(row(CODEX)?.textContent).toContain("Getting ChatGPT ready… 20 MB of 139 MB");
  // the words name the plan already: no name in front, and the clock while it downloads
  expect(row(CODEX)?.querySelector(".working-plan")).toBeNull();
  expect(row(CODEX)?.querySelector(".working")?.getAttribute("data-lead")).toBe("wait");
  // Claude's row stands as it was, its own Sign in still there
  expect(row(CLAUDE)?.textContent).toContain("Claude · needs a download of about 110 MB");
  await play(t, install(CODEX, READY), signIn(CODEX, { waiting: { since: AT, nudge: false } }));
  // a line without the plan's name gets it in front
  expect(row(CODEX)?.textContent).toContain("ChatGPT · Finish signing in with your browser.");
  await play(t, signIn(CODEX, SIGNED_IN));
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CODEX } }]);
  expect(installs(setups(t))).toEqual([]);
  expect(storeOf(t).state().chosen).toBe(CODEX);
});

test("both: the second waits with the queued line, and its sign-in goes once the first is ready and signed in, with no install sent over the first", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in to Claude");
  await press("Sign in to ChatGPT");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(row(CODEX)?.textContent).toContain("Your browser opens when ChatGPT is ready.");
  expect(row(CODEX)?.querySelector("button")?.textContent).toBe("Cancel");
  // Claude downloads, sets up, and signs in in the browser: ChatGPT waits through all of it
  await play(
    t,
    signIn(CLAUDE, "queued"),
    install(CLAUDE, { downloading: { received: 1, total: 112_000_000 } }),
  );
  await play(t, install(CLAUDE, "installing"));
  await play(t, install(CLAUDE, READY), signIn(CLAUDE, { waiting: { since: AT, nudge: false } }));
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(row(CODEX)?.textContent).toContain("Your browser opens when ChatGPT is ready.");
  await play(t, signIn(CLAUDE, SIGNED_IN));
  expect(setups(t)).toEqual([
    { start_sign_in: { agent: CLAUDE } },
    { start_sign_in: { agent: CODEX } },
  ]);
  expect(installs(setups(t))).toEqual([]);
  // the last pressed is the chosen plan
  expect(storeOf(t).state().chosen).toBe(CODEX);
});

test.each([
  [
    "its install failed",
    install(CLAUDE, { failed: { code: "agent.install_failed", hosts: [], log: [] } }),
  ],
  ["its sign-in was cancelled", signIn(CLAUDE, { failed: { code: "signin.cancelled", at: AT } })],
  ["its sign-in was cancelled from the line", signIn(CLAUDE, { signed_out: {} })],
])("the first plan's end releases the second: %s", async (_, end) => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in to Claude");
  await play(
    t,
    signIn(CLAUDE, "queued"),
    install(CLAUDE, { downloading: { received: 1, total: 112_000_000 } }),
  );
  await press("Sign in to ChatGPT");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  if (end.sign_in_changed !== undefined) await play(t, install(CLAUDE, READY));
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  await play(t, end);
  expect(setups(t)).toEqual([
    { start_sign_in: { agent: CLAUDE } },
    { start_sign_in: { agent: CODEX } },
  ]);
});

test("the waiting plan's Cancel stops its wait: nothing is sent for it, and its Sign in is back", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in to Claude");
  await press("Sign in to ChatGPT");
  await press(row(CODEX)?.querySelector("button") ?? null);
  expect(storeOf(t).state().queued).toBeNull();
  expect(row(CODEX)?.querySelector("button")?.textContent).toBe("Sign in to ChatGPT");
  await play(t, install(CLAUDE, READY), signIn(CLAUDE, SIGNED_IN));
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("a returning lawyer with ChatGPT alone signed in: the chosen plan is ChatGPT, not Claude", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, { missing: { download_bytes: 112_000_000 } }),
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, SIGNED_IN),
    ],
  });
  expect(storeOf(t).state().chosen).toBe(CODEX);
  // ChatGPT has nothing to offer; Claude's row still offers its own
  expect(rows().map((r) => r.dataset["agent"])).toEqual([CLAUDE]);
  expect(text()).toContain(
    "Your papers stay on this computer. Claude or ChatGPT sees the parts it reads to answer you.",
  );
});

test("a ready plan offered its sign-in leads with the tick, and its words name it once", async () => {
  await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  expect(row(CLAUDE)?.querySelector(".working")?.getAttribute("data-lead")).toBe("done");
  expect(row(CLAUDE)?.querySelector(".working-plan")).toBeNull();
  expect(row(CLAUDE)?.textContent).toContain("Claude is ready");
});
