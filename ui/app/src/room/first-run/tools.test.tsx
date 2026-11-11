// Clauseo's document tools on the first screen (stage D's runtime, one for the computer, SetupInfo's `python`): one quiet
// line under the two plan rows. Their size before, once for both plans and in neither plan's own; the download after a
// plan is ready, that plan usable meanwhile; the plans' offline words; a failure with Try again through a ready plan,
// hidden with none ready; Windows' block with no retry; and nothing for ready, unavailable or an older host's silence.
// A failure never changes a plan's row.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import type { PythonProgress, SetupEvent } from "../../transport/protocol.ts";
import {
  CLAUDE,
  CODEX,
  READY,
  allById,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  setups,
  signIn,
  text,
} from "./rig.tsx";

const tools = (python: PythonProgress): SetupEvent => ({
  setup_info: { keys_available: true, python },
});
const FRESH: SetupEvent[] = [
  install(CLAUDE, { missing: { download_bytes: 112_000_000 } }),
  signIn(CLAUDE, { signed_out: {} }),
  install(CODEX, { missing: { download_bytes: 139_000_000 } }),
  signIn(CODEX, { signed_out: {} }),
];
/** Claude ready (signed out), ChatGPT not here yet. */
const CLAUDE_READY: SetupEvent[] = [
  install(CLAUDE, READY),
  signIn(CLAUDE, { signed_out: {} }),
  install(CODEX, { missing: { download_bytes: 139_000_000 } }),
  signIn(CODEX, { signed_out: {} }),
];
const line = () => byId("tools-line");
const retry = () => byId("tools-line-retry");
const rowOf = (agent: string) =>
  document.querySelector<HTMLElement>(`[data-testid="plan-row"][data-agent="${agent}"]`);
const claudeRow = () => rowOf(CLAUDE);
const FAILED: PythonProgress = {
  failed: { cause: "failed", code: "agent.install_failed", log: ["failed"] },
};

test("an older host that says nothing of the document tools: no line", async () => {
  await mountFirstRun({ matters: [], first: FRESH });
  expect(line()).toBeNull();
  expect(text()).not.toContain("document tools");
});

test.each<[string, PythonProgress]>([
  ["unavailable", "unavailable"],
  ["ready", { ready: { version: "3.12.8" } }],
])("%s: no line", async (_, python) => {
  await mountFirstRun({ matters: [], first: [...FRESH, tools(python)] });
  expect(line()).toBeNull();
});

test("missing: their size, said once under both plans and in neither plan's own size", async () => {
  await mountFirstRun({
    matters: [],
    first: [...FRESH, tools({ missing: { download_bytes: 120_000_000 } })],
  });
  expect(allById("tools-line")).toHaveLength(1);
  expect(line()?.dataset["state"]).toBe("missing");
  expect(line()?.textContent).toBe("Also downloads Clauseo's document tools (120 MB).");
  expect(text().match(/document tools/g)).toHaveLength(1);
  // the plans' own sizes stay their own
  expect(claudeRow()?.textContent).toContain("Claude · needs a download of about 110 MB");
  expect(rowOf(CODEX)?.textContent).toContain("ChatGPT · needs a download of about 140 MB");
  // under the rows, before the fine print
  const rows = allById("plan-row");
  expect(rows.at(-1)!.compareDocumentPosition(line()!) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
    Node.DOCUMENT_POSITION_FOLLOWING,
  );
});

test("missing with no plan left to download: no size line", async () => {
  await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, READY),
      install(CODEX, READY),
      tools({ missing: { download_bytes: 120_000_000 } }),
    ],
  });
  expect(line()).toBeNull();
});

test("downloading after Claude is ready: done of size in whole MB with the plan rows' meter, Claude's row still ready to sign in; then installing says the full size", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [...CLAUDE_READY, tools({ downloading: { received: 42_000_000, total: 120_000_000 } })],
  });
  expect(line()?.dataset["state"]).toBe("downloading");
  expect(line()?.textContent).toContain("Downloading document tools… 42 MB of 120 MB");
  expect(line()?.querySelector(".fl-bar")).not.toBeNull();
  expect(line()?.querySelector('[role="status"]')).toBeNull();
  expect(claudeRow()?.textContent).toContain("Claude is ready");
  expect(claudeRow()?.querySelector("button")?.textContent).toBe("Sign in with Claude");
  await play(t, tools("installing"));
  expect(line()?.dataset["state"]).toBe("installing");
  expect(line()?.textContent).toContain("Downloading document tools… 120 MB of 120 MB");
  await play(t, tools({ ready: { version: "3.12.8" } }));
  expect(line()).toBeNull();
});

test("waiting offline: the plans' own offline words, with nothing to press", async () => {
  await mountFirstRun({
    matters: [],
    first: [
      ...CLAUDE_READY,
      tools({ waiting: { code: "agent.install_offline", hosts: [], log: [] } }),
    ],
  });
  expect(line()?.dataset["state"]).toBe("waiting");
  expect(line()?.textContent).toBe("You're offline. Setting up waits for the connection.");
  expect(line()?.querySelector("button")).toBeNull();
});

test("failed: the line and Try again, which sends RetryAgentInstall with the ready plan's id; Claude's row is unchanged", async () => {
  const t = await mountFirstRun({ matters: [], first: [...CLAUDE_READY, tools(FAILED)] });
  expect(line()?.dataset["state"]).toBe("failed");
  expect(line()?.textContent).toContain("Clauseo couldn't download its document tools.");
  expect(claudeRow()?.textContent).toContain("Claude is ready");
  await press(retry());
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE } }]);
});

test("failed with ChatGPT the only plan ready: Try again goes through ChatGPT", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, { missing: { download_bytes: 112_000_000 } }),
      install(CODEX, READY),
      signIn(CODEX, { signed_out: {} }),
      tools(FAILED),
    ],
  });
  await press(retry());
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CODEX } }]);
});

test("failed with no plan ready: the line, and no Try again", async () => {
  await mountFirstRun({ matters: [], first: [...FRESH, tools(FAILED)] });
  expect(line()?.textContent).toContain("Clauseo couldn't download its document tools.");
  expect(retry()).toBeNull();
});

test.each([["app_control"], ["applocker"]] as const)(
  "blocked by Windows (%s): its line and no button, since a retry can't help",
  async (by) => {
    await mountFirstRun({
      matters: [],
      first: [
        ...CLAUDE_READY,
        tools({ failed: { cause: "run_blocked", blocked_by: by, log: [] } }),
      ],
    });
    expect(line()?.dataset["state"]).toBe("blocked");
    expect(line()?.textContent).toBe("Windows blocked Clauseo's document tools on this computer.");
    expect(line()?.querySelector("button")).toBeNull();
  },
);

test("a start refused by an unknown policy, or one that failed its check: the plain failure with Try again", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      ...CLAUDE_READY,
      tools({ failed: { cause: "run_blocked", blocked_by: "unknown", log: [] } }),
    ],
  });
  expect(line()?.dataset["state"]).toBe("failed");
  await play(t, tools({ failed: { cause: "run_failed", log: [] } }));
  expect(line()?.dataset["state"]).toBe("failed");
  expect(retry()).not.toBeNull();
});
