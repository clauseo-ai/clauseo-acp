// The install's view from the host's words (views.ts): the download's size as "about" (the
// nearest 10 MB from 100 MB up, whole MB below), and a download's done MB never above its total.
import { expect, test } from "vitest";
import type { AgentSetup } from "../../store/setup.ts";
import { aboutMb, installView } from "./views.ts";

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

test("a failed set-up is a failed view marked setup; a failed download is not", () => {
  const failed = (code: "agent.install_setup_failed" | "agent.install_failed") =>
    installView(setup({ failed: { code, hosts: [], log: ["result: failed after 3 tries"] } }));
  expect(failed("agent.install_setup_failed")).toEqual({
    kind: "failed",
    log: ["result: failed after 3 tries"],
    setup: true,
  });
  expect(failed("agent.install_failed")).toEqual({
    kind: "failed",
    log: ["result: failed after 3 tries"],
  });
});

test("installing has no bytes; paused says the size it had, about", () => {
  expect(installView(setup("installing"))).toEqual({ kind: "installing" });
  expect(installView(setup("paused", 182))).toEqual({ kind: "missing", mb: 180 });
});
