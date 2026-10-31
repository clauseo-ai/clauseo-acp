import { expect, test } from "vitest";
import { FakeTransport } from "./transport/fake-transport.ts";
import { NullTransport, internalError, type HostTransport } from "./transport/host-transport.ts";
import { guessPlatform, hostPlatform } from "./platform.ts";

test("the host's report decides the platform; only a transport with no host guesses", async () => {
  expect(await hostPlatform(new FakeTransport({ platform: "windows" }))).toBe("windows");
  expect(await hostPlatform(new NullTransport())).toBe(guessPlatform());
});

test("a desktop transport whose display_info fails rejects instead of guessing", async () => {
  const failing = {
    kind: "tauri",
    displayInfo: () => Promise.reject(internalError("down")),
  } as unknown as HostTransport;
  await expect(hostPlatform(failing)).rejects.toMatchObject({ code: "internal" });
});
