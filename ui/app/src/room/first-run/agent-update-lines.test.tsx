// A plan the lawyer has, its install redone after an update of Clauseo (store/agent-update.ts), on the first screen
// (no matters) and in the room's composer line (a matter open). Stage D's update: Downloading, its ticks, Installing,
// Ready, each event with installed_before, and no Missing: "Updating {plan}… {done} MB of {total} MB", never "Add" or
// "Needs a download", and no sign-in prompt until Ready; after Ready the backend's own status check signs it in and
// the window sends no start_sign_in. A download that can't run says why with Try again. Missing with installed_before
// (#859's folder gone, or an update cancelled) is an update that stopped: "{plan}'s update stopped." and Try again,
// which sends the normal install_agent. A plan never installed keeps the first run's line and its Sign in; a firm's
// runtime keeps its own state.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import type { AgentId, InstallProgress, SetupEvent } from "../../transport/protocol.ts";
import {
  A,
  CLAUDE,
  READY,
  SIGNED_IN,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  setups,
  signIn,
  text,
} from "./rig.tsx";

const LOG = ["Clauseo download log", "agent: claude-acp", "result: failed after 3 tries"];
const HOSTS = ["registry.npmjs.org", "nodejs.org"];
const GONE: InstallProgress = { missing: { download_bytes: 160_000_000, installed_before: true } };

/** Stage D's install event for an added plan, flagged installed_before (not in today's bindings: agent-update.ts). */
const updating = (agent: AgentId, state: InstallProgress): SetupEvent =>
  ({ agent_install_progress: { agent, state, installed_before: true } }) as SetupEvent;
const down = (mb: number) => ({ downloading: { received: mb * 1_000_000, total: 160_000_000 } });

const foot = () => byId("install-foot");
const retry = () => byId("install-retry");

const never = () => {
  const words = text();
  expect(words).not.toContain("Needs a download");
  expect(words).not.toContain("needs a download");
  expect(words).not.toContain("Add Claude");
  expect(words).not.toContain("isn't installed");
  expect(byId("install-sign-in")).toBeNull();
  expect(byId("signin")).toBeNull();
};

test.each([
  ["the first screen", []],
  ["the room", [A]],
] as const)(
  "update on %s: Updating Claude with its progress, no sign-in prompt, and none sent after Ready",
  async (_, matters) => {
    const t = await mountFirstRun({ matters: [...matters], first: [updating(CLAUDE, down(0))] });
    expect(foot()?.dataset["state"]).toBe("downloading");
    expect(text()).toContain("Updating Claude… 0 MB of 160 MB");
    never();
    await play(t, updating(CLAUDE, down(80)));
    expect(text()).toContain("Updating Claude… 80 MB of 160 MB");
    await play(t, updating(CLAUDE, "installing"));
    expect(text()).toContain("Updating Claude…");
    never();
    await play(t, updating(CLAUDE, READY), signIn(CLAUDE, "checking"), signIn(CLAUDE, SIGNED_IN));
    expect(setups(t)).toEqual([]);
  },
);

test("update with a signed-out state pushed meanwhile (a host before stage D): still no sign-in prompt", async () => {
  const t = await mountFirstRun({
    matters: [A],
    first: [updating(CLAUDE, down(10)), signIn(CLAUDE, { signed_out: {} })],
  });
  expect(text()).toContain("Updating Claude… 10 MB of 160 MB");
  never();
  expect(setups(t)).toEqual([]);
});

test.each([
  ["the first screen", []],
  ["the room", [A]],
] as const)("update offline on %s: says it waits and tries again, with Try again", async (_, m) => {
  const t = await mountFirstRun({ matters: [...m], first: [updating(CLAUDE, down(10))] });
  await play(
    t,
    updating(CLAUDE, { waiting: { code: "agent.install_offline", hosts: HOSTS, log: LOG } }),
  );
  expect(foot()?.dataset["state"]).toBe("offline");
  expect(text()).toMatch(/waits for the connection|tries again by itself/);
  never();
  await press(retry());
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE } }]);
});

test.each([
  ["the first screen", []],
  ["the room", [A]],
] as const)("update failed on %s: the reason, and Try again sends retry", async (_, m) => {
  const t = await mountFirstRun({ matters: [...m], first: [updating(CLAUDE, down(10))] });
  await play(
    t,
    updating(CLAUDE, { failed: { code: "agent.install_failed", hosts: HOSTS, log: LOG } }),
  );
  expect(foot()?.dataset["state"]).toBe("failed");
  expect(text()).toContain("Claude's update couldn't be downloaded.");
  never();
  await press(retry());
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE } }]);
});

test("update on a network the firm blocks: the blocked words and their presses, never Add", async () => {
  const t = await mountFirstRun({ matters: [A], first: [updating(CLAUDE, down(10))] });
  await play(
    t,
    updating(CLAUDE, { failed: { code: "agent.install_blocked", hosts: HOSTS, log: LOG } }),
  );
  expect(foot()?.dataset["state"]).toBe("blocked");
  expect(text()).toContain("Your network is blocking the download.");
  expect(byId("install-proxy")).not.toBeNull();
  never();
});

test("update with a full disk: the space it needs, and Try again", async () => {
  const t = await mountFirstRun({ matters: [], first: [updating(CLAUDE, down(10))] });
  await play(
    t,
    updating(CLAUDE, {
      failed: {
        code: "agent.install_disk_full",
        need_mb: 400,
        free_mb: 120,
        hosts: HOSTS,
        log: LOG,
      },
    }),
  );
  expect(foot()?.dataset["state"]).toBe("disk");
  expect(retry()).not.toBeNull();
  never();
});

test.each([
  ["the first screen", []],
  ["the room", [A]],
] as const)(
  "installed before and missing on %s (cancelled, or #859): the update stopped, Try again installs, then Updating",
  async (_, m) => {
    const t = await mountFirstRun({ matters: [...m], first: [install(CLAUDE, GONE)] });
    expect(foot()?.dataset["state"]).toBe("update-stopped");
    expect(text()).toContain("Claude's update stopped.");
    never();
    expect(setups(t)).toEqual([]);
    await press(retry());
    expect(setups(t)).toEqual([{ install_agent: { agent: CLAUDE } }]);
    // a host before stage D says no flag on the download: it is still the update
    await play(t, install(CLAUDE, down(45)));
    expect(text()).toContain("Updating Claude… 45 MB of 160 MB");
    never();
  },
);

test("update paused: says so, and Try again goes on", async () => {
  const t = await mountFirstRun({ matters: [A], first: [updating(CLAUDE, down(10))] });
  await play(t, updating(CLAUDE, "paused"));
  expect(text()).toContain("Claude's update is paused.");
  never();
  await press(retry());
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE } }]);
});

test("in Hindi, the stopped update's line and Try again are the provisional Hindi", async () => {
  await mountFirstRun({ matters: [], language: "hi", first: [install(CLAUDE, GONE)] });
  expect(text()).toContain("Claude का अपडेट बीच में रुक गया।");
  expect(retry()?.textContent).toBe("फिर से प्रयास करें");
});

test("a plan never installed keeps the first run's line and its Sign in", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, { missing: { download_bytes: 160_000_000 } })],
  });
  expect(foot()?.dataset["state"]).toBe("missing");
  expect(text()).toContain("Claude · needs a download of about 160 MB");
  expect(text()).not.toContain("update");
  await press(byId("install-sign-in"));
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("a firm's runtime (managed by policy) shows no line, flagged or not", async () => {
  await mountFirstRun({ matters: [], first: [updating(CLAUDE, "managed")] });
  expect(foot()).toBeNull();
  expect(text()).not.toContain("update");
});
