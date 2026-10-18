import { transformAsync } from "@babel/core";
import { reactCompilerPreset } from "@vitejs/plugin-react";
import { expect, test } from "vitest";
// oxlint-disable-next-line import/default -- Vite gives `?raw` imports a default export that oxlint cannot see
import appSource from "./App.tsx?raw";

// The React Compiler skips a function it cannot compile instead of failing the build, so a broken
// Babel or compiler upgrade would only lose memoization. This runs App.tsx through the same preset
// as vite.config.ts and expects compiled output. (`memo_cache_sentinel` in a bundle proves
// nothing: React's own runtime defines it.)
test("the React Compiler preset compiles App.tsx", async () => {
  const result = await transformAsync(appSource, {
    filename: "App.tsx",
    babelrc: false,
    configFile: false,
    presets: [reactCompilerPreset().preset],
    parserOpts: { plugins: ["jsx", "typescript"] },
  });
  expect(result?.code).toContain("react/compiler-runtime");
});
