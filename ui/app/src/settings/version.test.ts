import { expect, test } from "vitest";
import tauriConf from "../../../../crates/desktop/tauri.conf.json" with { type: "json" };
import { appVersion } from "./app-version.ts";
import { APP_VERSION } from "./version.ts";

test("the compiled-in version is tauri.conf.json's, with release.yml's suffix when it sets one", () => {
  expect(tauriConf.version).toMatch(/^\d+\.\d+\.\d+/);
  expect(APP_VERSION).toBe(appVersion(tauriConf.version, process.env.CLAUSEO_VERSION_SUFFIX));
});
