// conversations.ts's agents list (R15's list_agents) over FakeTransport: read at start, read again on each
// settings_changed so a plan set up while the room is open joins the plan choice, and a reply a later read overtook
// dropped.
import { expect, test } from "vitest";
import { A, info, MADE, rig } from "./conversations-rig.ts";
import { DEFAULT_UI } from "./display-store.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import { WIRE, type AgentSummary } from "./host-next.ts";

const CLAUDE: AgentSummary = { id: "claude-acp", kind: "claude" };
const CODEX: AgentSummary = { id: "codex-acp", kind: "codex" };
const SETTINGS = { ui: DEFAULT_UI, locked: [] };

/** A NextHost with the matters, app, settings and agents parts, whose list_agents reads wait for the test's answers. */
function heldAgents() {
  const answers: ((agents: AgentSummary[]) => void)[] = [];
  const base = fakeNext({ matters: () => [detailed(A)], threads: () => [info(MADE)] });
  return {
    host: {
      next: {
        ...base,
        query: (query: Parameters<typeof base.query>[0]) =>
          query === WIRE.listAgents
            ? new Promise<{ agents: AgentSummary[] }>((resolve) =>
                answers.push((agents) => resolve({ agents })),
              )
            : base.query(query),
      },
    },
    answers,
  };
}

test("a plan set up while the room is open joins the choice: settings_changed reads list_agents again", async () => {
  const { host, answers } = heldAgents();
  const { transport, talks, settle } = await rig(host);
  talks.startNew();
  answers[0]?.([CLAUDE]);
  await settle();
  expect(talks.view().drafting).toBe(true);
  expect(talks.view().options).toEqual([]);
  transport.settingsChanged(SETTINGS);
  expect(answers).toHaveLength(2);
  answers[1]?.([CLAUDE, CODEX]);
  await settle();
  expect(talks.view().options.map((o) => o.value)).toEqual(["claude-acp", "codex-acp"]);
  expect(talks.view().agents).toEqual([CLAUDE, CODEX]);
});

test("a list_agents reply a later read overtook is dropped", async () => {
  const { host, answers } = heldAgents();
  const { transport, talks, settle } = await rig(host);
  transport.settingsChanged(SETTINGS);
  expect(answers).toHaveLength(2);
  // the later read answers first, then the start's read arrives late with the older list
  answers[1]?.([CLAUDE, CODEX]);
  await settle();
  answers[0]?.([CLAUDE]);
  await settle();
  expect(talks.view().agents).toEqual([CLAUDE, CODEX]);
  expect(talks.view().options).toHaveLength(2);
});

test("without the agents part, settings_changed reads nothing", async () => {
  const { host } = heldAgents();
  const { transport, settle } = await rig({
    ...host,
    next: { ...host.next, parts: ["matters", "app", "settings"] },
  });
  transport.settingsChanged(SETTINGS);
  await settle();
  expect(transport.only("next_query").filter((c) => c.query === WIRE.listAgents)).toEqual([]);
});
