// The blank app starts, renders #app-root under its CSP with no violation, sends that CSP with its
// scripts too, keeps its WebView2 data in the temporary folder, its WebView2 browser process got
// the arguments the e2e build passes, and the code-colouring worker runs and colours under the CSP.

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, type Response } from "@playwright/test";
import { test } from "./fixture.ts";
import { processTree } from "./memory.ts";

/**
 * The features in wry's own browser arguments: a copy of `WRY_DEFAULT_ARGS` in
 * `crates/desktop/src/e2e.rs`, which passes them again. Keep the two in step: the last step fails
 * if the browser process lacks any of these, which catches e2e.rs dropping one, but a feature added
 * only in e2e.rs goes unchecked.
 */
const WRY_DISABLED_FEATURES = ["msWebOOUI", "msPdfOOUI", "msSmartScreenProtection"];

/** `app.security.csp` as `crates/desktop/tauri.conf.json` writes it. */
const CONFIGURED_CSP: string = JSON.parse(
  readFileSync(new URL("../../../crates/desktop/tauri.conf.json", import.meta.url), "utf8"),
).app.security.csp;

/**
 * The colouring worker's path in the app, from the `ui/app/dist` the app embeds: the smoke job
 * builds it just before (`tauri build`), and a local run builds it with the app.
 */
function workerPath(): string {
  const assets = new URL("../../app/dist/assets/", import.meta.url);
  const workers = readdirSync(assets).filter((name) => /^worker-[\w-]+\.js$/.test(name));
  expect(workers, `one colouring worker in ${assets.pathname}`).toHaveLength(1);
  return `/assets/${workers[0]}`;
}

/** What the colouring worker answers (`ui/components`, `highlight/languages.ts` HighlightReply). */
type Reply = { id: number; lines: [number, string][][] | null };

/** The test's handle on the worker it starts, and what the page's CSP refused meanwhile. */
type Started = { __clauseoWorker?: Worker; __clauseoRefused?: string[] };

/** The arguments of a command line, without the quotes around each. */
function argumentsOf(commandLine: string): string[] {
  return commandLine
    .split(/\s+/)
    .filter((part) => part !== "")
    .map((part) => part.replace(/^"(.*)"$/, "$1"));
}

// One test, so the app starts once; each step names what failed.
/** What the page's CSP refused since it loaded, as the init script below records it. */
type Refused = { __clauseoRefused?: string[] };

test("the blank app starts, renders #app-root and runs as the e2e build says", async ({ app }) => {
  await test.step("#app-root is attached", async () => {
    await expect(app.page.locator("#app-root")).toBeAttached();
  });

  await test.step("no securitypolicyviolation event fires", async () => {
    // The page loaded before Playwright attached, so the listener goes in as an init script, which
    // runs before any page script, and the page loads again under it: Tauri's CSP header and the
    // same policy as a meta tag (ui/app/vite.config.ts, cspMeta).
    await app.page.addInitScript(() => {
      const refused: string[] = [];
      (window as Refused).__clauseoRefused = refused;
      document.addEventListener("securitypolicyviolation", (event) => {
        refused.push(
          `${event.effectiveDirective} ${event.blockedURI} ${event.sourceFile}:${event.lineNumber}`,
        );
      });
    });
    await app.page.reload({ waitUntil: "load" });
    await expect(app.page.locator("#app-root")).toBeAttached();
    // two frames, so what the first render and its effects asked for has been decided
    const refused = await app.page.evaluate(
      () =>
        new Promise<string[] | null>((resolve) =>
          requestAnimationFrame(() =>
            requestAnimationFrame(() => resolve((window as Refused).__clauseoRefused ?? null)),
          ),
        ),
    );
    expect(refused).toEqual([]);
  });

  await test.step("the page's scripts carry the CSP tauri.conf.json writes", async () => {
    // Tauri sends the CSP with the page only; the desktop crate's hook adds it to every script
    // (crates/desktop/src/csp.rs). This covers the scripts the blank page loads; the last step
    // covers the colouring worker.
    const scripts: Promise<[string, string | undefined]>[] = [];
    const onResponse = (response: Response) => {
      if (new URL(response.url()).pathname.endsWith(".js")) {
        scripts.push(
          response
            .allHeaders()
            .then((headers) => [response.url(), headers["content-security-policy"]]),
        );
      }
    };
    app.page.on("response", onResponse);
    await app.page.reload({ waitUntil: "load" });
    app.page.off("response", onResponse);
    const seen = await Promise.all(scripts);
    expect(seen.length).toBeGreaterThan(0);
    expect(seen).toEqual(seen.map(([url]) => [url, CONFIGURED_CSP]));
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
    // Port 0: the browser picks the port and writes it to DevToolsActivePort.
    expect(args).toContain("--remote-debugging-port=0");
    // Each feature in any --disable-features= argument, so a runtime that merges or reorders the
    // lists still passes.
    const disabled = args
      .filter((arg) => arg.startsWith("--disable-features="))
      .flatMap((arg) => arg.slice("--disable-features=".length).split(","));
    expect(disabled).toEqual(expect.arrayContaining(WRY_DISABLED_FEATURES));
    if (app.mode === "disable-gpu") {
      expect(args).toContain("--disable-gpu");
    } else {
      expect(args).not.toContain("--disable-gpu");
    }
  });

  await test.step("the colouring worker carries the CSP and colours a block under it", async () => {
    // A dedicated worker takes its policy from its own script's response, and the blank app never
    // starts it (the first code block does: start-worker.ts). So the step starts the built worker
    // from the page as start-worker.ts does, through a `clauseo-sanitize` policy (the page's CSP
    // takes a Worker URL only as a TrustedScriptURL; the app creates that policy only with its
    // first worker, so the name is free). Last, since the page then holds the policy.
    const path = workerPath();
    const workerResponse = app.page.waitForResponse(
      (response) => new URL(response.url()).pathname === path,
    );
    const workerStarted = app.page.waitForEvent("worker");
    await app.page.evaluate((url) => {
      const page = window as Started;
      // The init script of the CSP step already records the page's violations after each load.
      if (page.__clauseoRefused === undefined) {
        const refused: string[] = [];
        page.__clauseoRefused = refused;
        document.addEventListener("securitypolicyviolation", (event) => {
          refused.push(`page: ${event.effectiveDirective} ${event.blockedURI}`);
        });
      }
      type Factory = {
        createPolicy(
          name: string,
          rules: { createScriptURL: (url: string) => string },
        ): { createScriptURL(url: string): unknown };
      };
      const types = (window as { trustedTypes?: Factory }).trustedTypes;
      const policy = types?.createPolicy("clauseo-sanitize", {
        createScriptURL: (checked: string) => checked,
      });
      const scriptUrl = (policy?.createScriptURL(url) ?? url) as string;
      page.__clauseoWorker = new Worker(scriptUrl, { type: "module", name: "clauseo-highlight" });
    }, path);

    const response = await workerResponse;
    expect(response.status()).toBe(200);
    const headers = await response.allHeaders();
    expect(headers["content-type"]).toBe("text/javascript");
    expect(headers["content-security-policy"]).toBe(CONFIGURED_CSP);

    // Its own violations fire in its scope, not the page's: listen there before the first block,
    // which loads Shiki's grammar chunks and runs the colouring under require-trusted-types-for.
    const worker = await workerStarted;
    await worker.evaluate(() => {
      const scope = globalThis as { __clauseoRefused?: string[] } & typeof globalThis;
      const refused: string[] = [];
      scope.__clauseoRefused = refused;
      scope.addEventListener("securitypolicyviolation", (event) => {
        const violation = event as SecurityPolicyViolationEvent;
        refused.push(`worker: ${violation.effectiveDirective} ${violation.blockedURI}`);
      });
    });

    const reply = await app.page.evaluate(
      () =>
        new Promise<Reply>((resolve, reject) => {
          const started = (window as Started).__clauseoWorker;
          if (started === undefined) return reject(new Error("no worker"));
          started.addEventListener("message", (event: MessageEvent<Reply>) => resolve(event.data));
          started.addEventListener("error", (event) => reject(new Error(event.message)));
          started.postMessage({ id: 1, language: "typescript", code: "const x = 1; // note" });
        }),
    );
    expect(reply.id).toBe(1);
    // Coloured: some token took a role other than plain (0).
    expect(reply.lines?.flat().some(([role]) => role > 0)).toBe(true);

    const inWorker = await worker.evaluate(
      () => (globalThis as { __clauseoRefused?: string[] }).__clauseoRefused ?? null,
    );
    const inPage = await app.page.evaluate(() => (window as Started).__clauseoRefused ?? null);
    expect([...(inPage ?? []), ...(inWorker ?? [])]).toEqual([]);
    expect(inWorker).toEqual([]);
  });
});
