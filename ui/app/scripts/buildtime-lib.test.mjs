import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { brotliCompressSync, constants as zlib } from "node:zlib";
import { afterEach, describe, expect, it } from "vitest";
import {
  ROOT_TAG,
  initialJs,
  marked,
  parseRuns,
  resolveBin,
  scriptCommand,
  splitCommand,
} from "./buildtime-lib.mjs";

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

describe("marked", () => {
  const app = 'export function App() {\n  return <div id="app-root" />;\n}\n';

  it("adds the attribute right after ROOT_TAG", () => {
    expect(marked(app, 7)).toBe(
      'export function App() {\n  return <div id="app-root" data-buildtime="7" />;\n}\n',
    );
  });

  it("refuses an App.tsx where ROOT_TAG is missing or appears twice", () => {
    const twice = `// edits ${ROOT_TAG}\n${app}`;
    for (const text of ["export function App() { return null; }\n", twice]) {
      expect(() => marked(text, 1)).toThrow("ROOT_TAG must appear exactly once in src/App.tsx");
    }
  });
});

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

/** Writes `files` (path to text, or to an object written as JSON) under `dir`. */
function tree(dir, files) {
  for (const [path, content] of Object.entries(files)) {
    const full = join(dir, path);
    mkdirSync(join(full, ".."), { recursive: true });
    writeFileSync(full, typeof content === "string" ? content : JSON.stringify(content));
  }
}

describe("splitCommand", () => {
  it("splits a plain command on spaces", () => {
    expect(splitCommand("lint:types", "oxlint  --type-aware --format=json")).toEqual([
      "oxlint",
      "--type-aware",
      "--format=json",
    ]);
  });

  it("refuses shell syntax and a missing script", () => {
    for (const text of ["tsc && vite build", "oxlint | tee out", "vite 'build'", "A=1 vite"]) {
      expect(() => splitCommand("build", text)).toThrow("keep it one plain command");
    }
    expect(() => splitCommand("dev", undefined)).toThrow('ui/app/package.json has no script "dev"');
  });
});

describe("resolveBin and scriptCommand", () => {
  function workspace() {
    const ui = scratch();
    const app = join(ui, "app");
    tree(ui, {
      "package.json": { devDependencies: { oxfmt: "1" } },
      "node_modules/oxfmt/package.json": { name: "oxfmt", bin: "bin/oxfmt.js" },
      "app/package.json": {
        scripts: { typecheck: "tsc --noEmit", fmt: "oxfmt --check", bad: "tsc && vite" },
        devDependencies: { "@typescript/native": "npm:typescript@7", vite: "8" },
      },
      "app/node_modules/@typescript/native/package.json": {
        name: "typescript",
        bin: { tsc: "./bin/tsc" },
      },
      "app/node_modules/vite/package.json": { name: "vite", bin: { vite: "bin/vite.js" } },
    });
    return { ui, app };
  }

  it("finds a bin in the app's dependencies first, then in the ui root's", () => {
    const { ui, app } = workspace();
    expect(resolveBin("tsc", [app, ui])).toBe(join(app, "node_modules/@typescript/native/bin/tsc"));
    expect(resolveBin("oxfmt", [app, ui])).toBe(join(ui, "node_modules/oxfmt/bin/oxfmt.js"));
    expect(() => resolveBin("nope", [app, ui])).toThrow('provides the bin "nope"');
  });

  it("turns a package.json script into the bin's entry and its arguments", () => {
    const { ui, app } = workspace();
    expect(scriptCommand(app, ui, "typecheck")).toEqual({
      argv: [join(app, "node_modules/@typescript/native/bin/tsc"), "--noEmit"],
      text: "tsc --noEmit",
    });
    expect(() => scriptCommand(app, ui, "bad")).toThrow("keep it one plain command");
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
