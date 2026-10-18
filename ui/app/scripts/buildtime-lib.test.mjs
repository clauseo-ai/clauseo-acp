import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { brotliCompressSync, constants as zlib } from "node:zlib";
import { afterEach, describe, expect, it } from "vitest";
import {
  AppGuard,
  ConflictError,
  ROOT_TAG,
  appPaths,
  recoverApp,
  restoreFailure,
  tmpOf,
  unmarked,
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

describe("the App.tsx guard", () => {
  const REL = "ui/app/src/App.tsx";
  const ORIGINAL = 'export function App() {\n  return <div id="app-root" />;\n}\n';

  /** A repo with App.tsx holding `text`, and its paths. */
  function repo(text = ORIGINAL) {
    const paths = appPaths(scratch(), REL);
    tree(paths.app.slice(0, -REL.length), { [REL]: text });
    return paths;
  }

  const read = (path) => readFileSync(path, "utf8");

  it("marks and recovers back to the same bytes", () => {
    const paths = repo(marked(ORIGINAL, 3));
    tree(paths.base, { [REL]: ORIGINAL });
    expect(unmarked(marked(ORIGINAL, 3))).toBe(ORIGINAL);
    expect(recoverApp(paths)).toBe(`restored ${REL} from an interrupted buildtime run`);
    expect(read(paths.app)).toBe(ORIGINAL);
    expect(existsSync(paths.backup)).toBe(false);
    expect(existsSync(join(paths.base, "ui"))).toBe(false);
  });

  it("does nothing without a copy, and drops a copy of an unchanged file", () => {
    const paths = repo();
    expect(recoverApp(paths)).toBeUndefined();
    tree(paths.base, { [REL]: ORIGINAL });
    expect(recoverApp(paths)).toMatch(/^deleted the copy/);
    expect(read(paths.app)).toBe(ORIGINAL);
    expect(existsSync(paths.backup)).toBe(false);
  });

  it("deletes the temporary files of an interrupted write", () => {
    const paths = repo();
    writeFileSync(tmpOf(paths.app), "half");
    expect(recoverApp(paths)).toBeUndefined();
    expect(existsSync(tmpOf(paths.app))).toBe(false);
  });

  it("keeps a half-written App.tsx and its copy, and reports the conflict", () => {
    const paths = repo(ORIGINAL.slice(0, 20));
    tree(paths.base, { [REL]: ORIGINAL });
    expect(() => recoverApp(paths)).toThrow(
      `${REL} changed after an interrupted buildtime run; its saved copy is target/xtask/buildtime/${REL}`,
    );
    expect(read(paths.app)).toBe(ORIGINAL.slice(0, 20));
    expect(read(paths.backup)).toBe(ORIGINAL);
  });

  it("keeps a developer's edit made after a killed run, and a missing file is a conflict", () => {
    const edited = `${marked(ORIGINAL, 2)}// typed after the kill\n`;
    const paths = repo(edited);
    tree(paths.base, { [REL]: ORIGINAL });
    let error;
    try {
      recoverApp(paths);
    } catch (caught) {
      error = caught;
    }
    expect(error).toBeInstanceOf(ConflictError);
    expect(error.fix).toBe(
      `compare the two, keep what you want, then delete target/xtask/buildtime/${REL}`,
    );
    expect(read(paths.app)).toBe(edited);
    expect(read(paths.backup)).toBe(ORIGINAL);
    rmSync(paths.app);
    expect(() => recoverApp(paths)).toThrow("is missing after an interrupted buildtime run");
    expect(read(paths.backup)).toBe(ORIGINAL);
  });

  it("puts the original back after its edits and leaves no copy or temporary file", () => {
    const paths = repo();
    const guard = new AppGuard(paths);
    expect(read(paths.backup)).toBe(ORIGINAL);
    guard.write(marked(ORIGINAL, 1));
    guard.write(marked(ORIGINAL, 2));
    expect(read(paths.app)).toBe(marked(ORIGINAL, 2));
    expect(existsSync(tmpOf(paths.app))).toBe(false);
    guard.restore();
    guard.restore();
    expect(read(paths.app)).toBe(ORIGINAL);
    expect(existsSync(paths.backup)).toBe(false);
  });

  it("keeps a developer's edit made during the run, and both files", () => {
    const paths = repo();
    const guard = new AppGuard(paths);
    guard.write(marked(ORIGINAL, 1));
    const typed = `${marked(ORIGINAL, 1)}// typed during the run\n`;
    writeFileSync(paths.app, typed);
    expect(() => guard.write(marked(ORIGINAL, 2))).toThrow(
      `${REL} changed during this buildtime run; its saved copy is target/xtask/buildtime/${REL}`,
    );
    guard.restore();
    expect(read(paths.app)).toBe(typed);
    expect(read(paths.backup)).toBe(ORIGINAL);
  });

  it("keeps an edit made just before the restore", () => {
    const paths = repo();
    const guard = new AppGuard(paths);
    guard.write(marked(ORIGINAL, 1));
    writeFileSync(paths.app, "export {};\n");
    expect(() => guard.restore()).toThrow(ConflictError);
    expect(read(paths.app)).toBe("export {};\n");
    expect(read(paths.backup)).toBe(ORIGINAL);
  });

  it("turns a failed restore into a run failure that names the copy", () => {
    const paths = appPaths("/repo", REL);
    const conflict = new ConflictError("changed", "compare them");
    expect(restoreFailure(paths, conflict)).toEqual({ message: "changed", fix: "compare them" });
    const eperm = Object.assign(new Error("EPERM: operation not permitted, rename"), {
      code: "EPERM",
    });
    expect(restoreFailure(paths, eperm)).toEqual({
      message: `could not put back ${REL}: EPERM: operation not permitted, rename; its saved copy is target/xtask/buildtime/${REL}`,
      fix: `the next \`cargo xtask buildtime web\` restores ${REL} from target/xtask/buildtime/${REL}; or copy it back by hand`,
    });
  });

  it("refuses to start over an existing copy and leaves App.tsx alone", () => {
    const paths = repo();
    tree(paths.base, { [REL]: "an older copy" });
    expect(() => new AppGuard(paths)).toThrow(`a saved copy of ${REL} already exists`);
    expect(read(paths.backup)).toBe("an older copy");
    expect(read(paths.app)).toBe(ORIGINAL);
  });
});
