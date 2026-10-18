import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { brotliCompressSync, constants as zlib } from "node:zlib";
import { afterEach, describe, expect, it } from "vitest";
import { initialJs, parseRuns } from "./buildtime-lib.mjs";

const dirs = [];

/** A fresh folder under the OS temp dir, removed after each test. */
function scratch() {
  const dir = mkdtempSync(join(tmpdir(), "buildtime-lib-"));
  dirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function brotli(text) {
  return brotliCompressSync(Buffer.from(text), {
    params: { [zlib.BROTLI_PARAM_QUALITY]: 11 },
  }).length;
}

describe("parseRuns", () => {
  it("reads --runs and defaults to 5", () => {
    expect(parseRuns(["--runs", "3"])).toBe(3);
    expect(parseRuns([])).toBe(5);
  });

  it("rejects 0 and x", () => {
    expect(() => parseRuns(["--runs", "0"])).toThrow(
      "--runs takes a whole number of 1 or more, got 0",
    );
    expect(() => parseRuns(["--runs", "x"])).toThrow(
      "--runs takes a whole number of 1 or more, got x",
    );
  });
});

describe("initialJs", () => {
  it("sums the brotli size of the entry and both modulepreloads", () => {
    const dist = scratch();
    mkdirSync(join(dist, "assets"));
    const files = {
      "assets/index-a.js": "console.log('entry');".repeat(50),
      "assets/react-b.js": "export const react = 1;".repeat(80),
      "assets/dom-c.js": "export const dom = 2;".repeat(120),
    };
    for (const [file, text] of Object.entries(files)) {
      writeFileSync(join(dist, file), text);
    }
    writeFileSync(
      join(dist, "index.html"),
      `<!doctype html><html><head>
<script type="module" crossorigin src="/assets/index-a.js"></script>
<link rel="modulepreload" crossorigin href="/assets/react-b.js">
<link rel="modulepreload" crossorigin href="/assets/dom-c.js">
<link rel="stylesheet" href="/assets/index.css">
</head><body><div id="root"></div></body></html>`,
    );
    const sized = initialJs(dist);
    expect(sized.files).toEqual(["assets/index-a.js", "assets/react-b.js", "assets/dom-c.js"]);
    const want = Object.values(files).reduce((sum, text) => sum + brotli(text), 0);
    expect(sized.bytes).toBe(want);
  });

  it("fails when index.html has no module script", () => {
    const dist = scratch();
    writeFileSync(join(dist, "app.js"), "1");
    writeFileSync(
      join(dist, "index.html"),
      '<html><head><script src="/app.js"></script><link rel="modulepreload" href="/app.js"></head></html>',
    );
    expect(() => initialJs(dist)).toThrow("dist/index.html has no module script");
  });
});
