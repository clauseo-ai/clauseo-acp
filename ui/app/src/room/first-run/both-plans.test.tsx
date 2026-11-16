// The first screen offers both plans side by side and equal (the launch fix for a first run that offered Claude alone):
// a lead line, then Claude's row and ChatGPT's, each with its own size and "Sign in with {name}". Either plan alone
// installs and signs in, and the chosen plan follows the press; both pressed download side by side at once, each row
// with its own progress and Cancel, and sign in one at a time: a plan ready while the other's sign-in is under way says
// its browser opens after that one, and its sign-in follows by itself.
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
  expect(claude?.textContent).toBe("Sign in with Claude");
  expect(chatgpt?.textContent).toBe("Sign in with ChatGPT");
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
  // nothing chosen, neither signed in: the fine print names both plans (UD1-93)
  expect(text()).toContain(
    "Your papers stay on this computer. Claude or ChatGPT sees the parts it reads to answer you.",
  );
  expect(setups(t)).toEqual([]);
});

test("ChatGPT alone: its Sign in installs and signs in ChatGPT, and the chosen plan follows", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in with ChatGPT");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CODEX } }]);
  expect(storeOf(t).state().chosen).toBe(CODEX);
  // one plan in play, picked: the fine print names it alone (UD1-23)
  expect(text()).toContain(
    "Your papers stay on this computer. ChatGPT sees the parts it reads to answer you.",
  );
  expect(text()).not.toContain("Claude or ChatGPT sees");
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

test("both: ChatGPT pressed while Claude downloads sends ChatGPT's sign-in at once, nothing pauses Claude, and both rows show their own progress and Cancel", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in with Claude");
  await play(
    t,
    signIn(CLAUDE, "queued"),
    install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
  );
  await press("Sign in with ChatGPT");
  // desktop downloads ChatGPT beside Claude and holds its sign-in for its own download: no install over Claude's
  expect(setups(t)).toEqual([
    { start_sign_in: { agent: CLAUDE } },
    { start_sign_in: { agent: CODEX } },
  ]);
  expect(installs(setups(t))).toEqual([]);
  await play(
    t,
    signIn(CODEX, "queued"),
    install(CODEX, { downloading: { received: 90_000_000, total: 139_000_000 } }),
  );
  expect(rows().map((r) => r.dataset["agent"])).toEqual([CLAUDE, CODEX]);
  expect(row(CLAUDE)?.textContent).toContain("Getting Claude ready… 13 MB of 165 MB");
  expect(row(CODEX)?.textContent).toContain("Getting ChatGPT ready… 90 MB of 139 MB");
  expect(row(CLAUDE)?.querySelector("button")?.textContent).toBe("Cancel");
  expect(row(CODEX)?.querySelector("button")?.textContent).toBe("Cancel");
  // the last pressed is the chosen plan
  expect(storeOf(t).state().chosen).toBe(CODEX);
});

test("Cancel on one plan's row leaves the other's download and sign-in going", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in with Claude");
  await press("Sign in with ChatGPT");
  await play(
    t,
    signIn(CLAUDE, "queued"),
    install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
    signIn(CODEX, "queued"),
    install(CODEX, { downloading: { received: 90_000_000, total: 139_000_000 } }),
  );
  await press(row(CODEX)?.querySelector("button") ?? null);
  expect(setups(t).at(-1)).toEqual({ cancel_sign_in: { agent: CODEX } });
  expect(setups(t).filter((c) => Object.values(c)[0]?.agent === CLAUDE)).toEqual([
    { start_sign_in: { agent: CLAUDE } },
  ]);
  // desktop drops ChatGPT's sign-in and keeps its download; Claude's row is as it was
  await play(t, signIn(CODEX, { signed_out: {} }));
  expect(row(CODEX)?.textContent).toContain("Getting ChatGPT ready… 90 MB of 139 MB");
  expect(row(CLAUDE)?.textContent).toContain("Getting Claude ready… 13 MB of 165 MB");
  expect(row(CLAUDE)?.querySelector("button")?.textContent).toBe("Cancel");
});

test("ChatGPT ready while Claude's sign-in is under way: its row says its browser opens after Claude's, and desktop's sign-in follows with no press", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in with Claude");
  await press("Sign in with ChatGPT");
  await play(
    t,
    signIn(CLAUDE, "queued"),
    signIn(CODEX, "queued"),
    install(CODEX, READY),
    install(CLAUDE, READY),
    signIn(CLAUDE, { waiting: { since: AT, nudge: false } }),
  );
  expect(row(CODEX)?.textContent).toContain(
    "Your browser opens for ChatGPT after Claude's sign-in.",
  );
  expect(row(CODEX)?.querySelector("button")?.textContent).toBe("Cancel");
  await play(t, signIn(CLAUDE, SIGNED_IN), signIn(CODEX, { waiting: { since: AT, nudge: false } }));
  expect(row(CODEX)?.textContent).toContain("ChatGPT · Finish signing in with your browser.");
  // only the two presses' sign-ins went: desktop held ChatGPT's, one sign-in at a time
  expect(setups(t)).toEqual([
    { start_sign_in: { agent: CLAUDE } },
    { start_sign_in: { agent: CODEX } },
  ]);
});

test.each([
  ["signed in", signIn(CLAUDE, SIGNED_IN)],
  ["its sign-in was cancelled", signIn(CLAUDE, { failed: { code: "signin.cancelled", at: AT } })],
  ["its sign-in was cancelled from the line", signIn(CLAUDE, { signed_out: {} })],
])(
  "ChatGPT pressed while Claude signs in: its download starts at once, and its sign-in by itself once Claude's ends: %s",
  async (_, end) => {
    const t = await mountFirstRun({ matters: [], first: FRESH });
    await press("Sign in with Claude");
    await play(
      t,
      signIn(CLAUDE, "queued"),
      install(CLAUDE, READY),
      signIn(CLAUDE, { waiting: { since: AT, nudge: false } }),
    );
    await press("Sign in with ChatGPT");
    expect(setups(t)).toEqual([
      { start_sign_in: { agent: CLAUDE } },
      { install_agent: { agent: CODEX } },
    ]);
    await play(t, install(CODEX, { downloading: { received: 40_000_000, total: 139_000_000 } }));
    expect(row(CODEX)?.textContent).toContain("Getting ChatGPT ready… 40 MB of 139 MB");
    expect(row(CODEX)?.querySelector("button")?.textContent).toBe("Cancel");
    await play(t, install(CODEX, READY));
    expect(row(CODEX)?.textContent).toContain(
      "Your browser opens for ChatGPT after Claude's sign-in.",
    );
    expect(setups(t)).toHaveLength(2);
    await play(t, end);
    expect(setups(t)).toEqual([
      { start_sign_in: { agent: CLAUDE } },
      { install_agent: { agent: CODEX } },
      { start_sign_in: { agent: CODEX } },
    ]);
  },
);

test("a cancelled sign-in's row has no icon: no tick, since it isn't ready, and no clock, since nothing waits", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in with Claude");
  await play(
    t,
    install(CLAUDE, READY),
    signIn(CLAUDE, { failed: { code: "signin.cancelled", at: AT } }),
  );
  expect(row(CLAUDE)?.textContent).toContain("Sign in again");
  expect(row(CLAUDE)?.querySelector(".working-lead")?.getAttribute("data-lead")).toBe("none");
});

test("the waiting plan's Cancel stops its wait: nothing is sent for it, its download goes on, and its Sign in is back once it is here", async () => {
  const t = await mountFirstRun({ matters: [], first: FRESH });
  await press("Sign in with Claude");
  await play(t, install(CLAUDE, READY), signIn(CLAUDE, { waiting: { since: AT, nudge: false } }));
  await press("Sign in with ChatGPT");
  await play(t, install(CODEX, { downloading: { received: 40_000_000, total: 139_000_000 } }));
  await press(row(CODEX)?.querySelector("button") ?? null);
  expect(storeOf(t).state().queued).toBeNull();
  expect(row(CODEX)?.textContent).toContain("Getting ChatGPT ready… 40 MB of 139 MB");
  await play(t, install(CODEX, READY), signIn(CLAUDE, SIGNED_IN));
  expect(row(CODEX)?.querySelector("button")?.textContent).toBe("Sign in with ChatGPT");
  expect(setups(t)).toEqual([
    { start_sign_in: { agent: CLAUDE } },
    { install_agent: { agent: CODEX } },
  ]);
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
  expect(rows().map((r) => r.dataset["agent"])).toEqual([CLAUDE, CODEX]);
  expect(row(CODEX)?.textContent).toContain("ChatGPT is ready");
  // the only plan signed in is the one in play: the fine print names it alone (UD1-23)
  expect(text()).toContain(
    "Your papers stay on this computer. ChatGPT sees the parts it reads to answer you.",
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
