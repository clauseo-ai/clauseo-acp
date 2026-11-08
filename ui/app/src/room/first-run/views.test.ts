// The install's view from the host's words (views.ts): the download's size as "about" (the
// nearest 10 MB from 100 MB up, whole MB below), and a download's done MB never above its total.
import { expect, test } from "vitest";
import type { AgentSetup } from "../../store/setup.ts";
import { aboutMb, gateView, installView } from "./views.ts";
import type { SetupState } from "../../store/setup.ts";

const setup = (install: NonNullable<AgentSetup["install"]>, mb?: number): AgentSetup =>
  mb === undefined ? { install } : { install, mb };

test("about: 160 MB stays 160, 182 MB reads about 180, 165 MB about 170, under 100 MB whole", () => {
  expect(aboutMb(160)).toBe(160);
  expect(aboutMb(182)).toBe(180);
  expect(aboutMb(165)).toBe(170);
  expect(aboutMb(99)).toBe(99);
  expect(aboutMb(0)).toBe(0);
});

test("missing says the download's size, about, from the bytes the host gives", () => {
  expect(installView(setup({ missing: { download_bytes: 160_000_000 } }))).toEqual({
    kind: "missing",
    mb: 160,
  });
  expect(installView(setup({ missing: { download_bytes: 182_400_000 } }))).toEqual({
    kind: "missing",
    mb: 180,
  });
  expect(installView(setup({ missing: { download_bytes: 0 } }))).toEqual({
    kind: "missing",
    mb: 0,
  });
});

test("downloading carries the bytes for the bar and whole MB for the words: 45 MB of 160 MB", () => {
  expect(installView(setup({ downloading: { received: 45_000_000, total: 160_000_000 } }))).toEqual(
    {
      kind: "downloading",
      received: 45_000_000,
      total: 160_000_000,
      doneMb: 45,
      totalMb: 160,
    },
  );
});

test("downloading's done never runs above its total", () => {
  const view = installView(setup({ downloading: { received: 161_000_000, total: 160_000_000 } }));
  expect(view).toMatchObject({ kind: "downloading", doneMb: 160, totalMb: 160 });
});

test("installing has no bytes; paused says the size it had, about", () => {
  expect(installView(setup("installing"))).toEqual({ kind: "installing" });
  expect(installView(setup("paused", 182))).toEqual({ kind: "missing", mb: 180 });
});

const HOSTS = ["registry.npmjs.org", "nodejs.org"];

test("waiting: unreachable carries the hosts, offline stays offline", () => {
  expect(
    installView(setup({ waiting: { code: "agent.install_unreachable", hosts: HOSTS, log: [] } })),
  ).toEqual({ kind: "unreachable", hosts: HOSTS });
  expect(
    installView(setup({ waiting: { code: "agent.install_offline", hosts: HOSTS, log: [] } })),
  ).toEqual({ kind: "offline" });
});

test("a queued sign-in waits on a reconnect only while offline: unreachable is a network that is up", () => {
  const state = { keyOpen: false, pasting: false } as unknown as SetupState;
  const queued = (code: "agent.install_offline" | "agent.install_unreachable"): AgentSetup => ({
    install: { waiting: { code, hosts: HOSTS, log: [] } },
    signIn: "queued",
  });
  const clock = () => "";
  expect(gateView(queued("agent.install_offline"), state, clock)).toEqual({
    kind: "queued",
    offline: true,
  });
  expect(gateView(queued("agent.install_unreachable"), state, clock)).toEqual({
    kind: "queued",
    offline: false,
  });
});
