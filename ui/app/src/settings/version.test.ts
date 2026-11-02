import { expect, test } from "vitest";
import tauriConf from "../../../../crates/desktop/tauri.conf.json" with { type: "json" };
import { APP_VERSION } from "./version.ts";

test("the compiled-in version is tauri.conf.json's", () => {
  expect(tauriConf.version).toMatch(/^\d+\.\d+\.\d+/);
  expect(APP_VERSION).toBe(tauriConf.version);
});
