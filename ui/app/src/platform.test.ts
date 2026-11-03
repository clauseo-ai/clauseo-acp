import { expect, test } from "vitest";
import { FakeTransport } from "./transport/fake-transport.ts";
import { NullTransport, internalError, type HostTransport } from "./transport/host-transport.ts";
import { guessPlatform, hostDisplayInfo } from "./platform.ts";

test("the host's report decides the platform; only a transport with no host guesses", async () => {
  expect((await hostDisplayInfo(new FakeTransport({ platform: "windows" }))).platform).toBe(
    "windows",
  );
  expect(await hostDisplayInfo(new NullTransport())).toEqual({
    platform: guessPlatform(),
    release_date: null,
  });
});

test("the host's release date comes with its report; a transport with no host has none", async () => {
  expect(
    await hostDisplayInfo(new FakeTransport({ platform: "mac", releaseDate: "2026-11-03" })),
  ).toEqual({ platform: "mac", release_date: "2026-11-03" });
});

test("a desktop transport whose display_info fails rejects instead of guessing", async () => {
  const failing = {
    kind: "tauri",
    displayInfo: () => Promise.reject(internalError("down")),
  } as unknown as HostTransport;
  await expect(hostDisplayInfo(failing)).rejects.toMatchObject({ code: "internal" });
});
