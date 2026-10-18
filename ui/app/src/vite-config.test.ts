import { createServer } from "vite";
import { expect, test } from "vitest";
import tauriConf from "../../../crates/desktop/tauri.conf.json" with { type: "json" };
import { devCsp } from "../vite.config.ts";

// The React Compiler skips a function it cannot compile instead of failing the build, so a broken
// Babel or compiler setup would only lose memoization. This loads the real vite.config.ts and
// transforms App.tsx the way the dev server does, so it fails if the compiler is not wired in.
// (`memo_cache_sentinel` in a bundle proves nothing: React's own runtime defines it.) The path is
// relative to ui/app, where `pnpm -C ui/app run test` runs vitest.
test("vite.config.ts builds App.tsx with the React Compiler", async () => {
  const server = await createServer({
    configFile: "vite.config.ts",
    server: { middlewareMode: true, watch: null },
    logLevel: "silent",
  });
  try {
    const result = await server.transformRequest("/src/App.tsx");
    // The dev server rewrites `react/compiler-runtime` to its prebundled `react_compiler-runtime`.
    expect(result?.code).toMatch(/react[/_]compiler-runtime/);
  } finally {
    await server.close();
  }
});

test("the dev CSP is the production CSP plus inline script and style and Vite's websocket", () => {
  const production = tauriConf.app.security.csp;
  const dev = devCsp(production, 1420);
  expect(dev).toContain("script-src 'self' 'unsafe-inline';");
  expect(dev).toContain("style-src 'self' 'unsafe-inline';");
  expect(dev).toContain("require-trusted-types-for 'script'");
  // Every production directive survives, in order, with only those additions.
  const strip = (policy: string) =>
    policy.replaceAll(" 'unsafe-inline'", "").replace(" ws://localhost:1420", "");
  expect(strip(dev)).toBe(production.replaceAll(/\s*;\s*/g, "; "));
});

test("the dev CSP allows Vite's websocket on its own port and no other", () => {
  const dev = devCsp(tauriConf.app.security.csp, 51234);
  const connect = dev.split("; ").find((directive) => directive.startsWith("connect-src"));
  expect(connect?.endsWith(" ws://localhost:51234")).toBe(true);
  expect(dev).not.toContain("localhost:*");
});

test("the dev CSP refuses a production CSP without a directive it adds to", () => {
  expect(() => devCsp("default-src 'self'; style-src 'self'; connect-src ipc:", 1420)).toThrow(
    /no script-src/,
  );
});
