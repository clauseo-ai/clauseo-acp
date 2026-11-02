// Versions' panel, band and dialog stay out of the room's first load: walking the static imports from the room's entry
// (entry.ts) reaches use-versions.ts, Versions' one way in, and none of Versions' chunk, nor Versions' words table. A type-only import and a bare
// import() don't count; an import whose every name is a type still does (verbatimModuleSyntax keeps it as an import of
// the module).
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const room = resolve(here, "..");
const components = resolve(here, "../../../../components/src");

/** The static imports and re-exports of a module's source: never `import type`, `export type` or import(). */
function staticImports(source: string): string[] {
  const code = source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/.*$/gm, "$1");
  const out: string[] = [];
  const from = /(?:^|[;\n])\s*(import|export)\s+(type\s+)?([^;'"]*?)\s+from\s+["']([^"']+)["']/g;
  for (const m of code.matchAll(from)) if (m[2] === undefined && m[4] !== undefined) out.push(m[4]);
  const bare = /(?:^|[;\n])\s*import\s+["']([^"']+)["']/g;
  for (const m of code.matchAll(bare)) if (m[1] !== undefined) out.push(m[1]);
  return out;
}

/** A specifier's file, or null for a package or a file that isn't a module of source. */
function resolveFrom(file: string, spec: string): string | null {
  const bare = spec.replace(/\?.*$/, "");
  const path = bare.startsWith("@clauseo/components/")
    ? join(components, bare.slice("@clauseo/components/".length))
    : bare.startsWith(".")
      ? resolve(dirname(file), bare)
      : null;
  if (path === null || !/\.(ts|tsx)$/.test(path)) return null;
  if (!existsSync(path))
    throw new Error(`${relative(room, file)} imports ${spec}, which isn't there`);
  return path;
}

function reached(entry: string): Set<string> {
  const seen = new Set<string>([entry]);
  const todo = [entry];
  for (let file = todo.pop(); file !== undefined; file = todo.pop()) {
    for (const spec of staticImports(readFileSync(file, "utf8"))) {
      const next = resolveFrom(file, spec);
      if (next !== null && !seen.has(next)) {
        seen.add(next);
        todo.push(next);
      }
    }
  }
  return seen;
}

test("the walk sees through re-exports, skips type-only imports and import(), and keeps type-named imports", () => {
  expect(
    staticImports(
      [
        'import a from "./a.ts";',
        'import type { B } from "./b.ts";',
        'import { type C } from "./c.ts";',
        'export { D } from "./d.ts";',
        'export type { E } from "./e.ts";',
        'import "./f.css";',
        'const g = import("./g.ts");',
        '// import h from "./h.ts";',
        'import {\n  i,\n  j,\n} from "./ij.ts";',
      ].join("\n"),
    ),
  ).toEqual(["./a.ts", "./c.ts", "./d.ts", "./ij.ts", "./f.css"]);
});

test("the room's first load reaches use-versions.ts and none of Versions' chunk", () => {
  const files = reached(join(room, "entry.ts"));
  const names = new Set([...files].map((f) => relative(resolve(room, "../../.."), f)));
  // the walk did read the room: its parts' modules are in
  expect(names.has("app/src/room/room.tsx")).toBe(true);
  expect(names.has("app/src/room/versions/use-versions.ts")).toBe(true);
  // and the same walk from Versions' chunk does reach its parts
  const chunk = [...reached(join(here, "versions-chunk.ts"))].map((f) => relative(components, f));
  expect(chunk).toContain("clauseo/version-panel.tsx");
  for (const kept of [
    "components/src/clauseo/version-panel.tsx",
    "components/src/clauseo/version-band.tsx",
    "components/src/clauseo/restore-dialog.tsx",
    "components/src/clauseo/version-page.tsx",
    "components/src/clauseo/version-where.tsx",
    "app/src/room/versions/versions-parts.tsx",
    "app/src/room/versions/versions-chunk.ts",
    "app/src/room/versions/controller.ts",
    "app/src/room/versions/conversations.ts",
    "app/src/room/versions/reading.ts",
    "app/src/room/versions/timeline.ts",
    "app/src/room/versions/words.ts",
    "app/src/room/versions/state.ts",
    "app/src/room/versions/seeking.ts",
    // Versions' words and what the matter allows: the toolbar's few words, and its Not protected mark, have homes of
    // their own (words-toolbar.ts, not-protected.tsx)
    "components/src/lib/words-versions.ts",
    "components/src/clauseo/allowed.tsx",
  ])
    expect(names.has(kept), kept).toBe(false);
});
