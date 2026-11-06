// UD1, Done-when 1: each install state and each install code's screen, on the first screen's foot and in the room's
// composer; the bar moving only on events; each install button sending its command; Managed with no foot.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import {
  A,
  CLAUDE,
  READY,
  allById,
  byId,
  counts,
  install,
  mountFirstRun,
  play,
  press,
  setups,
  signIn,
  stubClipboard,
  text,
} from "./rig.tsx";

const LOG = ["Clauseo download log", "agent: claude-acp", "result: failed after 3 tries"];
const HOSTS = ["registry.npmjs.org", "nodejs.org"];

const foot = () => byId("install-foot");
const bar = () => foot()?.querySelector('[role="meter"]') ?? null;

test("install: missing says the size and sends nothing (desktop starts the install itself)", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, { missing: { download_bytes: 182_400_000 } })],
  });
  expect(foot()?.dataset["state"]).toBe("missing");
  expect(text()).toContain("Getting Claude ready. About 182 MB.");
  expect(setups(t)).toEqual([]);
});

test("install: the bar shows received of total with no number and no time, and moves only on events", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, { downloading: { received: 20_000_000, total: 200_000_000 } })],
  });
  expect(foot()?.dataset["state"]).toBe("downloading");
  const before = bar()?.getAttribute("aria-valuenow");
  expect(before).toBe("10");
  // the meter speaks the words, never a number; the line shows no percent and no clock time
  expect(bar()?.getAttribute("aria-valuetext")).toBe("Getting Claude ready. About 200 MB.");
  expect(foot()?.closest(".working")?.textContent ?? "").not.toMatch(/%|\d:\d\d|seconds|minute/);
  // nothing moves it but the host: waiting changes nothing
  await new Promise((resolve) => setTimeout(resolve, 30));
  expect(bar()?.getAttribute("aria-valuenow")).toBe(before);
  await play(t, install(CLAUDE, { downloading: { received: 100_000_000, total: 200_000_000 } }));
  expect(bar()?.getAttribute("aria-valuenow")).toBe("50");
});

test("install: installing is a calm wait with no bar and no time, the pen writing", async () => {
  await mountFirstRun({ matters: [], first: [install(CLAUDE, "installing")] });
  expect(foot()?.dataset["state"]).toBe("installing");
  expect(bar()).toBeNull();
  expect(text()).toContain("Setting up Claude on this computer.");
  expect(document.querySelector('.working [data-lead="pen"]')).not.toBeNull();
});

const OFFLINE = install(CLAUDE, {
  waiting: { code: "agent.install_offline", hosts: HOSTS, log: LOG },
});

test("install: dl-offline waits for the connection and resumes by itself", async () => {
  const t = await mountFirstRun({ matters: [], first: [OFFLINE] });
  expect(foot()?.dataset["state"]).toBe("offline");
  expect(text()).toContain("You're offline. Setting up waits for the connection.");
  await play(t, install(CLAUDE, { downloading: { received: 1, total: 200_000_000 } }));
  expect(foot()?.dataset["state"]).toBe("downloading");
  expect(setups(t)).toEqual([]);
});

test("install: dl-offline offers Try again and Use the system proxy, Try again first", async () => {
  await mountFirstRun({ matters: [], first: [OFFLINE] });
  expect(foot()?.dataset["state"]).toBe("offline");
  expect(byId("install-retry")?.textContent).toBe("Try again");
  expect(byId("install-proxy")?.textContent).toBe("Use the system proxy");
  const names = [...(foot()?.querySelectorAll("button") ?? [])].map((b) => b.textContent);
  expect(names).toEqual(["Try again", "Use the system proxy"]);
});

test("install: dl-offline's Try again sends RetryAgentInstall with no proxy (missing means false)", async () => {
  const t = await mountFirstRun({ matters: [], first: [OFFLINE] });
  await press(byId("install-retry"));
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE } }]);
});

test("install: dl-offline's Use the system proxy sends RetryAgentInstall with use_system_proxy true", async () => {
  const t = await mountFirstRun({ matters: [], first: [OFFLINE] });
  await press(byId("install-proxy"));
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE, use_system_proxy: true } }]);
});

test("install: dl-blocked offers the system proxy (RetryAgentInstall with use_system_proxy) and copies the IT message", async () => {
  const writeText = stubClipboard();
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, { failed: { code: "agent.install_blocked", hosts: HOSTS, log: LOG } })],
  });
  expect(foot()?.dataset["state"]).toBe("blocked");
  expect(text()).toContain("Your network is blocking the download.");
  await press(byId("install-proxy"));
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE, use_system_proxy: true } }]);
  expect(byId("install-ask-it")?.textContent).toBe("Ask your IT team");
  await press(byId("install-ask-it"));
  expect(writeText).toHaveBeenCalledWith(
    "Clauseo needs to download Claude to work. Please allow these addresses on the firm's network: registry.npmjs.org, nodejs.org.",
  );
  expect(byId("install-ask-it")?.textContent).toBe("Message copied");
});

test("install: dl-disk says what it needs and what is free, and Try again sends RetryAgentInstall", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, {
        failed: {
          code: "agent.install_disk_full",
          need_mb: 420,
          free_mb: 96,
          hosts: HOSTS,
          log: LOG,
        },
      }),
    ],
  });
  expect(foot()?.dataset["state"]).toBe("disk");
  expect(text()).toContain("Claude needs 420 MB free on this computer, and 96 MB is free.");
  await press(byId("install-retry"));
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE } }]);
});

test("install: dl-error offers Try again and Details, which copies the event's log and says so", async () => {
  const writeText = stubClipboard();
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, { failed: { code: "agent.install_failed", hosts: HOSTS, log: LOG } })],
  });
  expect(foot()?.dataset["state"]).toBe("failed");
  expect(text()).toContain("Claude couldn't be downloaded.");
  expect(byId("install-proxy")?.textContent).toBe("Use the system proxy");
  await press("Details");
  expect(writeText).toHaveBeenCalledWith(LOG.join("\n"));
  expect(text()).toContain(
    "Claude couldn't be downloaded. The log is copied, with nothing from your matters in it.",
  );
  await press(byId("install-retry"));
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE } }]);
  // a new failure is said fresh, its log not yet copied
  await play(
    t,
    install(CLAUDE, "installing"),
    install(CLAUDE, { failed: { code: "agent.install_failed", hosts: HOSTS, log: LOG } }),
  );
  expect(text()).not.toContain("The log is copied");
});

test("install: dl-error's Use the system proxy sends RetryAgentInstall with use_system_proxy true", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, { failed: { code: "agent.install_failed", hosts: HOSTS, log: LOG } })],
  });
  await press(byId("install-proxy"));
  expect(setups(t)).toEqual([{ retry_agent_install: { agent: CLAUDE, use_system_proxy: true } }]);
});

test("install: ready says Claude is ready on the first screen, and counts agent_ready once", async () => {
  const t = await mountFirstRun({ matters: [], first: [install(CLAUDE, "installing")] });
  await play(t, install(CLAUDE, READY), install(CLAUDE, READY));
  expect(foot()).toBeNull();
  expect(text()).toContain("Claude is ready");
  expect(counts(t).filter((c) => c.onboarding_step?.step === "agent_ready")).toHaveLength(1);
});

test("install: managed (IT installed it) shows no foot", async () => {
  await mountFirstRun({ matters: [], first: [install(CLAUDE, "managed")] });
  expect(foot()).toBeNull();
  expect(text()).not.toContain("Getting Claude ready");
  expect(text()).not.toContain("Claude is ready");
});

test("install: the foot follows into the room, on the composer's working line, until the install is ready", async () => {
  const t = await mountFirstRun({
    matters: [A],
    first: [
      install(CLAUDE, { downloading: { received: 5, total: 200_000_000 } }),
      signIn(CLAUDE, { signed_out: {} }),
    ],
  });
  expect(allById("install-foot")).toHaveLength(1);
  expect(foot()?.closest("[data-composer]")).not.toBeNull();
  await play(t, install(CLAUDE, "installing"));
  expect(foot()?.dataset["state"]).toBe("installing");
  await play(t, install(CLAUDE, READY));
  expect(foot()).toBeNull();
});

test("install: each install code's screen counts error_shown with its code, once", async () => {
  const t = await mountFirstRun({ matters: [] });
  for (const code of [
    "agent.install_blocked",
    "agent.install_disk_full",
    "agent.install_failed",
  ] as const)
    await play(
      t,
      install(CLAUDE, { failed: { code, hosts: HOSTS, log: LOG } }),
      install(CLAUDE, { failed: { code, hosts: HOSTS, log: LOG } }),
    );
  await play(
    t,
    install(CLAUDE, { waiting: { code: "agent.install_offline", hosts: HOSTS, log: LOG } }),
  );
  expect(counts(t).filter((c) => c.error_shown !== undefined)).toEqual([
    { error_shown: { code: "agent.install_blocked" } },
    { error_shown: { code: "agent.install_disk_full" } },
    { error_shown: { code: "agent.install_failed" } },
    { error_shown: { code: "agent.install_offline" } },
  ]);
});
