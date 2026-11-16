import { expect, test } from "vitest";
import { appVersion } from "./app-version.ts";

test("a build with no suffix shows the plain version", () => {
  expect(appVersion("0.1.0", undefined)).toBe("0.1.0");
  expect(appVersion("0.1.0", "")).toBe("0.1.0");
});

test("a dry run's suffix follows the version as a pre-release", () => {
  expect(appVersion("0.0.0", "dev.036d043b8")).toBe("0.0.0-dev.036d043b8");
});

test("a suffix that isn't a semver pre-release is refused", () => {
  for (const bad of ["dev 1", "dev/1", "dev..1", ".dev", "dév"]) {
    expect(() => appVersion("0.1.0", bad), bad).toThrow(/CLAUSEO_VERSION_SUFFIX/);
  }
});
