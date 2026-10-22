import { defineConfig } from "@playwright/test";

/**
 * The real app on Windows, driven over CDP (app-windows/fixture.ts). No browser is downloaded or
 * launched: the tests attach to the app's own WebView2.
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
