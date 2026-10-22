import { defineConfig } from "@playwright/test";

/**
 * The real app on Windows, driven over CDP (app-windows/fixture.ts). No browser is downloaded or
 * launched: the tests attach to the app's own WebView2.
 */
export default defineConfig({
  testDir: "app-windows",
  // One app at a time: the single-instance plugin ends a second launch.
  workers: 1,
  fullyParallel: false,
  retries: 0,
  forbidOnly: true,
  timeout: 120_000,
  expect: { timeout: 30_000 },
  reporter: [["list"]],
});
