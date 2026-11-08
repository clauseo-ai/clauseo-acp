// The planted test of the geometry bans in ui/app/.oxlintrc.json. getPointAtLength and getTotalLength
// measure a path again on every call (0.9 ms a read for the pen's ink in Chrome): the pen's table of
// 2,380 reads held the first run's "Setting up" off the screen for about 8 s on the Windows VM
// (PR #775). The pinned oxlint lints a scratch tree under the OS temp dir (inside ui/app, the
// `oxlint` step of `cargo xtask check` would lint the planted files too) that holds a copy of the
// config and one use per file, and must report each one, while pen-mark.tsx's one getTotalLength
// and the pen tests' stubs pass. ui/e2e, which oxlint does not lint, is read for them instead.

import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { resolveBin } from "./buildtime-lib.mjs";
import { runTool } from "./buildtime-proc.mjs";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const E2E_DIR = resolve(APP_DIR, "..", "e2e");
/** How long one oxlint run over the scratch tree may take; it takes well under a second. */
const OXLINT_TIMEOUT_MS = 60_000;

const PROPERTIES = "eslint(no-restricted-properties)";
const COMMENTS = "eslint(no-warning-comments)";
const POINT = "getPointAtLength measures the whole path again on every call";
const TOTAL = "getTotalLength measures the whole path on every call";
const TAURI = "Tauri's injected globals are reached only under src/transport/";

const PATH = `const p = document.querySelector("path") as SVGPathElement;`;

/** Each planted use, as its file in the tree ui/app's oxlint sees, and what oxlint must say. */
const BANNED = {
  "src/point.tsx": [`${PATH}\np.getPointAtLength(1);`, PROPERTIES, POINT],
  "src/total.ts": [`${PATH}\np.getTotalLength();`, PROPERTIES, TOTAL],
  "src/computed.ts": [`${PATH}\np["getPointAtLength"](1);`, PROPERTIES, POINT],
  "src/destructured.ts": [
    `const { getTotalLength } = SVGPathElement.prototype;`,
    PROPERTIES,
    TOTAL,
  ],
  "src/optional.ts": [`document.querySelector("path")?.getPointAtLength(1);`, PROPERTIES, POINT],
  "components/src/clauseo/other.tsx": [`${PATH}\np.getTotalLength();`, PROPERTIES, TOTAL],
  "src/transport/geometry.ts": [`${PATH}\np.getPointAtLength(1);`, PROPERTIES, POINT],
  "scripts/geometry.mjs": [`const p = globalThis.path;\np.getTotalLength();`, PROPERTIES, TOTAL],
  "src/disable.ts": [
    `${PATH}\n// oxlint-disable-next-line no-restricted-properties\np.getPointAtLength(1);`,
    COMMENTS,
    "",
  ],
};

/** pen-mark.tsx: its one getTotalLength passes; getPointAtLength and Tauri's globals stay banned. */
const PEN_MARK = {
  "components/src/clauseo/pen-mark.tsx": `${PATH}\np.getTotalLength();\np.getPointAtLength(1);\nconsole.log(window.__TAURI__);`,
};

/** The pen's tests stub both on the prototypes, to prove the pen never walks the path. */
const STUBS = {
  "components/src/clauseo/pen-mark.test.tsx": `const proto = SVGElement.prototype as unknown as Record<string, unknown>;\nproto["getTotalLength"] = () => 1000;\nproto["getPointAtLength"] = (k: number) => ({ x: k, y: 0 });`,
  "src/room/pen-axe.test.tsx": `const proto = SVGElement.prototype as unknown as Record<string, unknown>;\nproto["getTotalLength"] = () => 1000;\nproto["getPointAtLength"] = (k: number) => ({ x: k, y: 0 });`,
};

/**
 * Ways past the bans that oxlint 1.87.0 cannot see, pinned so that a release that catches one fails
 * here. A name built at run time: no-restricted-properties matches only a literal name. A disable
 * comment that names no rule: it turns off every rule below it, no-warning-comments included.
 */
const UNCAUGHT = {
  "src/built.ts": `${PATH}\nconst k = "getPoint" + "AtLength";\n(p as unknown as Record<string, (n: number) => void>)[k]?.(1);`,
  "src/disable-all.ts": `${PATH}\n// ${"oxlint"}-disable-next-line\np.getPointAtLength(1);`,
};

const BANS = new Set([PROPERTIES, COMMENTS]);

const dirs = [];

/** Lints `files` (a path in the tree to its text) in a scratch tree with a copy of the app's .oxlintrc.json. */
async function lint(files) {
  const dir = mkdtempSync(join(tmpdir(), "oxlint-geometry-"));
  dirs.push(dir);
  writeFileSync(join(dir, ".oxlintrc.json"), readFileSync(join(APP_DIR, ".oxlintrc.json")));
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, name)), { recursive: true });
    writeFileSync(join(dir, name), `${text}\nexport {};\n`);
  }
  const oxlint = resolveBin("oxlint", [APP_DIR]);
  const result = await runTool([oxlint, "--format", "json", "."], {
    cwd: dir,
    timeoutMs: OXLINT_TIMEOUT_MS,
  });
  expect(result.timedOut, result.output).toBe(false);
  const byFile = {};
  const { diagnostics } = JSON.parse(result.output);
  expect(diagnostics.filter((diagnostic) => diagnostic.code === undefined)).toEqual([]);
  for (const diagnostic of diagnostics) {
    if (BANS.has(diagnostic.code)) {
      const name = diagnostic.filename.replaceAll("\\", "/").replace(/^\.\//, "");
      (byFile[name] ??= []).push(diagnostic);
    }
  }
  return byFile;
}

afterAll(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("the geometry bans", () => {
  let findings;

  beforeAll(async () => {
    const banned = Object.entries(BANNED).map(([name, [text]]) => [name, text]);
    findings = await lint({ ...Object.fromEntries(banned), ...PEN_MARK, ...STUBS, ...UNCAUGHT });
  }, OXLINT_TIMEOUT_MS);

  it.each(Object.entries(BANNED))("report %s", (name, [, code, says]) => {
    const found = (findings[name] ?? []).filter((diagnostic) => diagnostic.code === code);
    expect(found, JSON.stringify(findings[name] ?? [])).toHaveLength(1);
    expect(`${found[0].message} ${found[0].help ?? ""}`).toContain(says);
  });

  it("pass pen-mark.tsx's getTotalLength, and still report its getPointAtLength and Tauri's globals", () => {
    const said = (findings["components/src/clauseo/pen-mark.tsx"] ?? [])
      .map((diagnostic) => `${diagnostic.message} ${diagnostic.help ?? ""}`)
      .toSorted();
    expect(said).toHaveLength(2);
    expect(said.some((text) => text.includes(POINT))).toBe(true);
    expect(said.some((text) => text.includes(TAURI))).toBe(true);
    expect(said.some((text) => text.includes(TOTAL))).toBe(false);
  });

  it.each(Object.keys(STUBS))("pass the stubs in %s", (name) => {
    expect(findings[name]).toBeUndefined();
  });

  it.each(Object.keys(UNCAUGHT))("still miss %s, which oxlint 1.87.0 cannot see", (name) => {
    expect(findings[name]).toBeUndefined();
  });
});

/** The uses of either name in `text`, comments included: ui/e2e has no reason to name them at all. */
function geometryUses(text) {
  return [...text.matchAll(/\bget(?:PointAtLength|TotalLength)\b/g)].map((m) => m[0]);
}

describe("ui/e2e, which oxlint does not lint", () => {
  it("finds a planted use", () => {
    expect(geometryUses(`await page.evaluate(() => path.getPointAtLength(3));`)).toEqual([
      "getPointAtLength",
    ]);
  });

  it("names neither getPointAtLength nor getTotalLength", () => {
    const files = readdirSync(E2E_DIR, { recursive: true })
      .map((file) => String(file).replaceAll("\\", "/"))
      .filter(
        (file) => !file.split("/").includes("node_modules") && /\.(?:[cm]?[jt]sx?)$/.test(file),
      );
    expect(files.length).toBeGreaterThan(0);
    const uses = files.filter(
      (file) => geometryUses(readFileSync(join(E2E_DIR, file), "utf8")).length > 0,
    );
    expect(uses).toEqual([]);
  });
});
