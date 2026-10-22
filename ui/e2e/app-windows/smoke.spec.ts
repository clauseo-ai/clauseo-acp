// The blank app starts, renders #app-root, keeps its WebView2 data in the temporary folder, and its
// WebView2 browser process got the arguments the e2e build passes.

import { existsSync } from "node:fs";
import { join } from "node:path";
import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { processTree } from "./memory.ts";

/** wry's own browser arguments, which `crates/desktop/src/e2e.rs` passes again. */
const WRY_DEFAULT_ARGS = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/** The arguments of a command line, without the quotes around each. */
function argumentsOf(commandLine: string): string[] {
  return commandLine
    .split(/\s+/)
    .filter((part) => part !== "")
    .map((part) => part.replace(/^"(.*)"$/, "$1"));
}

// One test, so the app starts once; each step names what failed.
test("the blank app starts, renders #app-root and runs as the e2e build says", async ({ app }) => {
  await test.step("#app-root is attached", async () => {
    await expect(app.page.locator("#app-root")).toBeAttached();
  });

  await test.step("WebView2 keeps its data in the temporary folder", () => {
    // WebView2 keeps its profile in <data folder>/EBWebView. Without it, WebView2 ignored
    // WEBVIEW2_USER_DATA_FOLDER and wrote to the app's real data folder.
    expect(existsSync(join(app.userDataFolder, "EBWebView"))).toBe(true);
  });

  await test.step("the browser process got the e2e build's arguments", async () => {
    // The browser process is the one msedgewebview2.exe without --type=; the others are its
    // renderer, GPU and utility processes.
    const browsers = (await processTree(app.pid)).filter(
      (entry) =>
        entry.name === "msedgewebview2.exe" &&
        entry.commandLine !== null &&
        !argumentsOf(entry.commandLine).some((arg) => arg.startsWith("--type=")),
    );
    expect(browsers).toHaveLength(1);
    const args = argumentsOf(browsers[0]?.commandLine ?? "");
    expect(args).toContain(`--remote-debugging-port=${app.port}`);
    expect(args).toContain(WRY_DEFAULT_ARGS);
    if (app.mode === "disable-gpu") {
      expect(args).toContain("--disable-gpu");
    } else {
      expect(args).not.toContain("--disable-gpu");
    }
  });
});
