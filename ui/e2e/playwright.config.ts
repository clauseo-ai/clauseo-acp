import { defineConfig } from "@playwright/test";

/**
 * The real app on Windows, driven over CDP (app-windows/fixture.ts). No browser is downloaded or
 * launched: the tests attach to the app's own WebView2.
 *
 * The file name picks the project: `*.test.ts` is a pure unit test that runs on any OS,
 * `*.spec.ts` drives the real app and runs on Windows only. The unit tests use Playwright's runner
 * rather than ui/app's vitest, so this package needs no second test dependency.
 */
export default defineConfig({
  testDir: "app-windows",
  projects: [
    // Pure functions of the harness, on any OS: `pnpm run test:unit`.
    { name: "unit", testMatch: "**/*.test.ts" },
    // The real app, Windows only: `pnpm test`.
    { name: "app", testMatch: "**/*.spec.ts" },
  ],
  // One app at a time: the single-instance plugin ends a second launch.
  workers: 1,
  fullyParallel: false,
  retries: 0,
  forbidOnly: true,
  // Above the sum of the harness's own timeouts, so one of those fires first and names its step.
  timeout: 600_000,
  // The wait for #app-root: in normal mode on windows-2025, the navigation to the app has taken
  // from 2 s to over 30 s. A slow start shows in msToRoot instead of failing the run.
  expect: { timeout: 90_000 },
  reporter: [["list"]],
});
