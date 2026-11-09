// #859: the chosen plan's install went missing after Clauseo installed it (desktop's Missing says installed_before).
// The line says so and offers "Install {plan}", which sends the normal install_agent; its size, progress, offline wait
// and failures then show as for any install. A plan never installed keeps the first run's line and its Sign in; a
// firm's runtime (managed) or a blocked network keeps that state's own words, with no Install.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import type { InstallProgress } from "../../transport/protocol.ts";
import {
  A,
  CLAUDE,
  CODEX,
  READY,
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

const foot = () => byId("install-foot");
const again = () => byId("install-again");
const planButton = (name: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find(
    (b) => b.textContent?.trim() === name,
  ) ?? null;

test("reinstall: Claude gone after an install says so, and Install Claude sends install_agent, then the normal line", async () => {
  const t = await mountFirstRun({ matters: [], first: [install(CLAUDE, GONE)] });
  expect(foot()?.dataset["state"]).toBe("reinstall");
  expect(text()).toContain("Claude isn't installed on this computer any more.");
  expect(text()).not.toContain("Needs a download");
  expect(byId("install-sign-in")).toBeNull();
  expect(again()?.textContent).toBe("Install Claude");
  expect(setups(t)).toEqual([]);
  await press(again());
  expect(setups(t)).toEqual([{ install_agent: { agent: CLAUDE } }]);
  // the normal install's line: its size and progress, the offline wait, a failure with Details
  await play(t, install(CLAUDE, { downloading: { received: 45_000_000, total: 160_000_000 } }));
  expect(foot()?.dataset["state"]).toBe("downloading");
  expect(text()).toContain("Getting Claude ready… 45 MB of 160 MB");
  expect(again()).toBeNull();
  await play(
    t,
    install(CLAUDE, { waiting: { code: "agent.install_offline", hosts: HOSTS, log: LOG } }),
  );
  expect(foot()?.dataset["state"]).toBe("offline");
  await play(
    t,
    install(CLAUDE, { failed: { code: "agent.install_failed", hosts: HOSTS, log: LOG } }),
  );
  expect(foot()?.dataset["state"]).toBe("failed");
  expect(text()).toContain("Details");
  expect(again()).toBeNull();
});

test("reinstall: ChatGPT gone after an install says so by its plan's name, and Install ChatGPT installs Codex", async () => {
  const t = await mountFirstRun({
    matters: [A],
    first: [install(CLAUDE, READY), install(CODEX, GONE), signIn(CODEX, { signed_out: {} })],
  });
  await press(planButton("ChatGPT"));
  expect(foot()?.dataset["state"]).toBe("reinstall");
  expect(text()).toContain("ChatGPT isn't installed on this computer any more.");
  expect(text()).not.toContain("Codex");
  expect(again()?.textContent).toBe("Install ChatGPT");
  await press(again());
  expect(setups(t)).toContainEqual({ install_agent: { agent: CODEX } });
});

test("reinstall: in Hindi, the line and the button are the provisional Hindi", async () => {
  await mountFirstRun({ matters: [], language: "hi", first: [install(CLAUDE, GONE)] });
  expect(text()).toContain("Claude अब इस कंप्यूटर पर इंस्टॉल नहीं है।");
  expect(again()?.textContent).toBe("Claude इंस्टॉल करें");
});

test("reinstall: a plan never installed keeps the first run's line and its Sign in, with no Install", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, { missing: { download_bytes: 160_000_000 } })],
  });
  expect(foot()?.dataset["state"]).toBe("missing");
  expect(text()).toContain("Needs a download of about 160 MB");
  expect(text()).not.toContain("isn't installed");
  expect(again()).toBeNull();
  await press(byId("install-sign-in"));
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("reinstall: a firm's runtime (managed by policy) shows no line and no Install", async () => {
  await mountFirstRun({ matters: [], first: [install(CLAUDE, "managed")] });
  expect(foot()).toBeNull();
  expect(again()).toBeNull();
  expect(text()).not.toContain("isn't installed");
});

test("reinstall: Install on a network the firm blocks shows the blocked words and their presses, no Install", async () => {
  const t = await mountFirstRun({ matters: [], first: [install(CLAUDE, GONE)] });
  await press(again());
  await play(
    t,
    install(CLAUDE, { failed: { code: "agent.install_blocked", hosts: HOSTS, log: LOG } }),
  );
  expect(foot()?.dataset["state"]).toBe("blocked");
  expect(text()).toContain("Your network is blocking the download.");
  expect(byId("install-proxy")).not.toBeNull();
  expect(again()).toBeNull();
});
