// Every stylesheet under ui/ (ui/app's src and ui/components' src) ships with a window the desktop loads, and every class
// of one that a release page's JavaScript names has a rule in that page's CSS. A screen imports only the sheets of the
// parts it shows (styles/index.css, settings.css, a lazy chunk's own sheet), while the review gallery and the parts
// gallery import every sheet through clauseo/parts.css, so a sheet left out of a page's imports looks right in review
// and ships unstyled: the toolbar's plan card did (no padding, its labels and figures run together, no bars), with the
// limit notes (limits.css), the protection line and the Not protected note (edge.css) and the update notice
// (rail-notice.css). The first test walks each page's import graph from its HTML entry (vite.config.ts' PAGES); the
// second builds the release pages and checks each one's classes.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createBuilder, type Rolldown } from "vite";
import { expect, test, vi } from "vitest";
import { PAGES } from "../../vite.config.ts";

const UI = fileURLToPath(new URL("../../../", import.meta.url));
const ROOTS = [join(UI, "app/src"), join(UI, "components/src")];

/** Sheets no window loads, each with why: the dev-only pages' own, and the parts only the galleries draw. */
const NOT_SHIPPED: Record<string, string> = {
  "app/src/gallery/gallery.css": "the dev-only gallery page (gallery.html), not built for release",
  "app/src/gallery/parts/parts.css": "the dev-only parts gallery (parts.html)",
  "app/src/review/review.css": "the dev-only copy review page (review.html)",
  "components/src/clauseo/parts.css":
    "every part's sheet gathered for the galleries and tests; a window imports its own",
  "components/src/clauseo/cards.css":
    "the excerpt card, provenance and History: the room passes onVersions, so RoomToolbar's History never draws",
  "components/src/clauseo/ik.css":
    "Indian Kanoon in the thread: the parts gallery only, not in the app yet",
  "components/src/clauseo/reader.css":
    "the judgment reader and its pane: the parts gallery only, not in the app yet",
  "components/src/clauseo/turned-page.css":
    "the turned page over the thread: the parts gallery only, not in the app yet",
};

/** Every file under `dir` but node_modules, as an absolute path. */
function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const at = join(dir, name);
    return name === "node_modules" ? [] : statSync(at).isDirectory() ? files(at) : [at];
  });
}
const ALL = ROOTS.flatMap(files);
const SHEETS = ALL.filter((f) => f.endsWith(".css"));
const key = (f: string) => relative(UI, f);

/** `spec` imported from `from`, as a file under ui/, or null for a package or a file not there. A query (`?url`) is
 * dropped: a lazy chunk's sheet is imported so (versions-chunk.ts, changes-chunk.ts). */
function resolveImport(from: string, spec: string): string | null {
  const bare = spec.split("?")[0]!;
  const base = bare.startsWith("@clauseo/components/")
    ? join(UI, "components/src", bare.slice("@clauseo/components/".length))
    : bare.startsWith(".")
      ? resolve(dirname(from), bare)
      : null;
  if (base === null) return null;
  return [base, `${base}.ts`, `${base}.tsx`].find((f) => ALL.includes(f)) ?? null;
}

const IMPORTS =
  /(?:import|export)\s[^;]*?from\s+"([^"]+)"|import\s+"([^"]+)"|import\(\s*"([^"]+)"\s*\)|@import\s+"([^"]+)"/g;

/** Every file under ui/ that `entry` reaches through its imports, static and dynamic, and a sheet's @import. */
function reached(entry: string, into = new Set<string>()): Set<string> {
  if (into.has(entry) || /\.test\./.test(entry)) return into;
  into.add(entry);
  for (const m of readFileSync(entry, "utf8").matchAll(IMPORTS)) {
    const next = resolveImport(entry, m[1] ?? m[2] ?? m[3] ?? m[4] ?? "");
    if (next !== null) reached(next, into);
  }
  return into;
}

test("every sheet under ui/ is reached from a window the desktop loads, or is on NOT_SHIPPED's list", () => {
  const shipped = new Set<string>();
  for (const page of Object.values(PAGES)) {
    const src = /<script[^>]*src="\/([^"]+)"/.exec(readFileSync(page, "utf8"))?.[1];
    expect(src, page).toBeDefined();
    for (const f of reached(join(dirname(page), src!))) shipped.add(key(f));
  }
  expect(Object.keys(PAGES).sort()).toEqual(["index", "settings"]);
  const orphans = SHEETS.map(key).filter((f) => !shipped.has(f) && NOT_SHIPPED[f] === undefined);
  expect(orphans).toEqual([]);
  // a listed sheet that a window now loads, or that is gone, comes off the list
  for (const f of Object.keys(NOT_SHIPPED)) {
    expect(SHEETS.map(key), f).toContain(f);
    expect(shipped.has(f), `${f} ships now: take it off NOT_SHIPPED`).toBe(false);
  }
});

/** Classes a page names that its own CSS leaves to another sheet on purpose, each with why it draws nothing there. */
const NOT_DRAWN: Record<string, Record<string, string>> = {
  "index.html": {
    // RoomToolbar's own Versions popover (cards.css' History, on NOT_SHIPPED's list): the room always passes onVersions
    // (use-versions.ts), so the toolbar opens Versions' timeline and this branch never draws
    "history-card": "cards.css",
    "history-list": "cards.css",
    "history-row": "cards.css",
    "history-what": "cards.css",
    "history-time": "cards.css",
    "history-foot": "cards.css",
  },
  "settings.html": {
    // settings' words key, not a class: words-plan-card.ts' wordsOf("plan-card")
    "plan-card": "plan-card.css",
    // the same for words-matter-form.ts' wordsOf("matter-form")
    "matter-form": "matter-form.css",
    // a font size's name in the type scale's list (tailwind-merge's config), not a class
    "tile-title": "wall.css",
  },
};

/** Every sheet a window may load, by its path under ui/, with its text. */
const shippable = (): [string, string][] =>
  SHEETS.filter((f) => NOT_SHIPPED[key(f)] === undefined).map((f) => [
    key(f),
    readFileSync(f, "utf8"),
  ]);

/** A sheet's own class names with a hyphen: distinctive enough that JavaScript naming one means the class. */
const classesOf = (css: string) => [
  ...new Set(
    [...css.matchAll(/\.([a-z][a-z0-9]*-[a-z0-9-]*[a-z0-9])(?![\w-])/g)].map((m) => m[1]!),
  ),
];
const escape = (name: string) => name.replaceAll("-", "\\-");
/** `name` as a class in a JSX className string, a word of it between quotes, backticks or spaces. */
const named = (js: string, name: string) =>
  new RegExp(`[\\s"'\`]${escape(name)}[\\s"'\`]`).test(js);
const ruled = (css: string, name: string) => new RegExp(`\\.${escape(name)}(?![\\w-])`).test(css);

test(
  "every sheet's class the release build's JavaScript names has a rule in its page's CSS",
  { timeout: 60_000 },
  async () => {
    // as `vite build` runs it: vitest's NODE_ENV=test would make the build a development one
    vi.stubEnv("NODE_ENV", "production");
    const builder = await createBuilder({
      configFile: "vite.config.ts",
      logLevel: "silent",
      build: { write: false },
    });
    const parts = shippable().map(([path, css]) => [path, classesOf(css)] as const);
    const missing: Record<string, string[]> = {};
    let pages = 0;
    for (const environment of Object.values(builder.environments)) {
      const out = (await builder.build(environment)) as
        | Rolldown.RolldownOutput
        | Rolldown.RolldownOutput[];
      const files = (Array.isArray(out) ? out : [out]).flatMap((o) => o.output);
      const page = files.find((f) => f.fileName.endsWith(".html"))!.fileName;
      pages += 1;
      const js = files.flatMap((f) => (f.type === "chunk" ? [f.code] : [])).join("\n");
      const css = files
        .flatMap((f) =>
          f.type === "asset" && f.fileName.endsWith(".css") ? [String(f.source)] : [],
        )
        .join("\n");
      const allowed = NOT_DRAWN[page] ?? {};
      for (const [path, classes] of parts)
        for (const name of classes)
          if (named(js, name) && !ruled(css, name) && allowed[name] === undefined)
            (missing[page] ??= []).push(`${path}: .${name}`);
      // a class allowed here that the page now draws, or no longer names, comes off the list
      for (const name of Object.keys(allowed))
        expect(
          named(js, name) && !ruled(css, name),
          `${page}: ${name} is off NOT_DRAWN's list`,
        ).toBe(true);
    }
    expect(pages).toBe(2);
    expect(missing).toEqual({});
  },
);
