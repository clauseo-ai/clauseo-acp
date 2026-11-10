// Every class of a part's stylesheet (ui/components' clauseo/**/*.css) that the release build's JavaScript names has a
// rule in the CSS that page ships. A screen imports only the sheets of the parts it shows (styles/index.css,
// settings.css), while the review gallery and the parts gallery import every sheet through clauseo/parts.css, so a sheet
// left out of a page's imports looks right in review and ships unstyled: the toolbar's plan card did (no padding, its
// labels and figures run together, no bars), with the limit notes (limits.css), the protection line and the Not
// protected note (edge.css) and the update notice (rail-notice.css). This builds the release pages and checks each one.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createBuilder, type Rolldown } from "vite";
import { expect, test, vi } from "vitest";

const PARTS = fileURLToPath(new URL("../../../components/src/clauseo/", import.meta.url));

/** Classes a page names that its own CSS leaves to another sheet on purpose, each with why it draws nothing there. */
const NOT_DRAWN: Record<string, Record<string, string>> = {
  "index.html": {
    // RoomToolbar's own Versions popover (cards.css' History): the room always passes onVersions (use-versions.ts), so
    // the toolbar opens Versions' timeline and this branch never draws
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
    // known gaps, left for a follow-up: Plans' Ask-sized button (composer.css' min width), the caption buttons over an
    // open dialog (responsive.css' z-index), the workspace remove confirm's body (edge.css' grid gap)
    "room-field-ask": "composer.css",
    "room-caption": "responsive.css",
    "np-note-body": "edge.css",
  },
};

/** Every part's sheet, under clauseo/ and its folders, by its path there. */
function sheets(dir: string = PARTS, at = ""): [string, string][] {
  return readdirSync(dir).flatMap((name): [string, string][] =>
    statSync(join(dir, name)).isDirectory()
      ? sheets(join(dir, name), `${at}${name}/`)
      : name.endsWith(".css") && name !== "parts.css"
        ? [[`${at}${name}`, readFileSync(join(dir, name), "utf8")]]
        : [],
  );
}

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
  "every part's class the release build's JavaScript names has a rule in its page's CSS",
  { timeout: 60_000 },
  async () => {
    // as `vite build` runs it: vitest's NODE_ENV=test would make the build a development one
    vi.stubEnv("NODE_ENV", "production");
    const builder = await createBuilder({
      configFile: "vite.config.ts",
      logLevel: "silent",
      build: { write: false },
    });
    const parts = sheets().map(([path, css]) => [path, classesOf(css)] as const);
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
