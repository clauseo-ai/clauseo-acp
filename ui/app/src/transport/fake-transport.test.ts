import { describe, expect, test } from "vitest";
import { FakeTransport } from "./fake-transport.ts";

describe("FakeTransport", () => {
  test("open_external, open_file and reveal are recorded and succeed unscripted", async () => {
    const t = new FakeTransport();
    await t.openExternal("mailto:a@example.com");
    await t.openFile("ref-1");
    await t.reveal({ matter: "m-1" });
    expect(t.calls).toEqual([
      { call: "open_external", url: "mailto:a@example.com" },
      { call: "open_file", file: "ref-1" },
      { call: "reveal", target: { matter: "m-1" } },
    ]);
  });

  test("a scripted refusal rejects as a HostError; a thrown Error becomes internal", async () => {
    const declined = { code: "transport.unauthorized", message: "declined", retryable: false };
    const t = new FakeTransport({
      openExternal: () => {
        throw declined;
      },
      reveal: () => Promise.reject(new Error("no finder")),
    });
    await expect(t.openExternal("https://example.com")).rejects.toEqual(declined);
    await expect(t.reveal("app_data")).rejects.toEqual({
      code: "internal",
      message: "no finder",
      retryable: false,
    });
  });
});
