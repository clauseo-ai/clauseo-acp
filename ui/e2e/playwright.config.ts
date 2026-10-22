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
  // Fits the worst case: setup 60 s, the settle wait 30 s plus one snapshot, the snapshots (30 s,
  // or 90 s with the working set) and taskkill 30 s.
  timeout: 300_000,
  expect: { timeout: 30_000 },
  reporter: [["list"]],
});
