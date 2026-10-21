import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants as zlib } from "node:zlib";
import { afterEach, describe, expect, it } from "vitest";
import {
  BACKUP_DIR,
  ROOT_TAG,
  appPaths,
  initialJs,
  marked,
  minNode,
  nodeAtLeast,
  parseRuns,
  recoverApp,
  resolveBin,
  scriptCommand,
  splitCommand,
  unmarked,
} from "./buildtime-lib.mjs";
import { restoreFailure, tmpOf } from "./guard.mjs";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");

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

describe("nodeAtLeast", () => {
  it("compares major, minor and patch as numbers", () => {
    const min = "20.5.0";
    expect(nodeAtLeast("20.5.0", min)).toBe(true);
    expect(nodeAtLeast("v20.5.1", min)).toBe(true);
    expect(nodeAtLeast("22.0.0", min)).toBe(true);
    expect(nodeAtLeast("20.10.0", min)).toBe(true);
    expect(nodeAtLeast("20.4.9", min)).toBe(false);
    expect(nodeAtLeast("20.0.0", min)).toBe(false);
    expect(nodeAtLeast("v18.20.0", min)).toBe(false);
  });
});

describe("minNode", () => {
  it("reads the version of engines.node in ui/package.json", () => {
    const ui = resolve(APP_DIR, "..");
    const range = JSON.parse(readFileSync(join(ui, "package.json"), "utf8")).engines?.node;
    expect(`>=${minNode(ui)}`).toBe(range);
  });

  it("reads a scratch package.json, and refuses a range it cannot compare", () => {
    const ui = scratch();
    tree(ui, { "package.json": { engines: { node: ">=20.5.0" } } });
    expect(minNode(ui)).toBe("20.5.0");
    for (const [engines, shown] of [
      [{ node: "^20.5.0" }, '"^20.5.0"'],
      [{}, "undefined"],
    ]) {
      tree(ui, { "package.json": { engines } });
      expect(() => minNode(ui)).toThrow(
        `${join(ui, "package.json")} has engines.node ${shown}; buildtime reads only ">=<major>.<minor>.<patch>"`,
      );
    }
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

describe("the real ui/app/package.json", () => {
  it("has the four scripts buildtime runs, each one plain command with an installed bin", () => {
    for (const script of ["typecheck", "lint:types", "build", "dev"]) {
      const { argv } = scriptCommand(APP_DIR, resolve(APP_DIR, ".."), script);
      expect(existsSync(argv[0])).toBe(true);
    }
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

describe("buildtime's part of the App.tsx guard", () => {
  const REL = "ui/app/src/App.tsx";
  const ORIGINAL = 'export function App() {\n  return <div id="app-root" />;\n}\n';

  /** A repo with App.tsx holding `text` and its saved copy holding ORIGINAL, and its paths. */
  function interrupted(text) {
    const paths = appPaths(scratch(), REL);
    tree(paths.file.slice(0, -REL.length), { [REL]: text });
    tree(paths.base, { [REL]: ORIGINAL });
    return paths;
  }

  const read = (path) => readFileSync(path, "utf8");

  it("keeps the copy under target/xtask/buildtime and names buildtime's temporary files", () => {
    const paths = appPaths("/repo", REL);
    expect(paths.backup).toBe(join("/repo", BACKUP_DIR, REL));
    expect(paths.backupRel).toBe(`target/xtask/buildtime/${REL}`);
    expect(tmpOf(paths.file, paths.tool)).toBe(join("/repo", "ui/app/src/.App.tsx.buildtime.tmp"));
  });

  it("marks and recovers back to the same bytes", () => {
    const paths = interrupted(marked(ORIGINAL, 3));
    expect(unmarked(marked(ORIGINAL, 3))).toBe(ORIGINAL);
    expect(recoverApp(paths)).toBe(`restored ${REL} from an interrupted buildtime run`);
    expect(read(paths.file)).toBe(ORIGINAL);
    expect(existsSync(paths.backup)).toBe(false);
    expect(existsSync(join(paths.base, "ui"))).toBe(false);
  });

  it("keeps a developer's edit made after a killed run, and a mark twice", () => {
    for (const text of [
      `${marked(ORIGINAL, 2)}// typed after the kill\n`,
      marked(marked(ORIGINAL, 2), 3),
    ]) {
      const paths = interrupted(text);
      expect(() => recoverApp(paths)).toThrow(
        `${REL} changed after an interrupted buildtime run; its saved copy is target/xtask/buildtime/${REL}`,
      );
      expect(read(paths.file)).toBe(text);
      expect(read(paths.backup)).toBe(ORIGINAL);
    }
  });

  it("names cargo xtask buildtime web in the fix of a failed restore", () => {
    const eperm = new Error("EPERM: operation not permitted, rename");
    expect(restoreFailure(appPaths("/repo", REL), eperm)).toEqual({
      message: `could not put back ${REL}: EPERM: operation not permitted, rename; its saved copy is target/xtask/buildtime/${REL}`,
      fix: `the next \`cargo xtask buildtime web\` restores ${REL} from target/xtask/buildtime/${REL}; or copy it back by hand`,
    });
  });
});
