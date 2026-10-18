// The pure parts of buildtime.mjs, kept apart so buildtime-lib.test.mjs can test them: argument
// parsing and sizing the initial JS of a build.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { brotliCompressSync, constants as zlib } from "node:zlib";

/**
 * The text the HMR probe edits in src/App.tsx. App.tsx names this constant in a comment; change
 * the two together.
 */
export const ROOT_TAG = '<div id="app-root"';

/** `original` with `data-buildtime="<n>"` added right after the first ROOT_TAG. */
export function marked(original, n) {
  const at = original.indexOf(ROOT_TAG);
  if (at === -1) {
    throw new Error(`src/App.tsx has no ${ROOT_TAG}`);
  }
  const end = at + ROOT_TAG.length;
  return `${original.slice(0, end)} data-buildtime="${n}"${original.slice(end)}`;
}

/** The `--runs N` of `argv`, 5 when absent. Throws on anything but a whole number of 1 or more. */
export function parseRuns(argv) {
  const at = argv.indexOf("--runs");
  if (at === -1) {
    return 5;
  }
  const value = argv[at + 1];
  const runs = /^\d+$/.test(value ?? "") ? Number(value) : Number.NaN;
  if (!Number.isSafeInteger(runs) || runs < 1) {
    throw new Error(`--runs takes a whole number of 1 or more, got ${value}`);
  }
  return runs;
}

/**
 * The initial JS of the build in `dist`: the `<script type="module" src>` entry plus every
 * `<link rel="modulepreload" href>` of `dist/index.html`, each brotli-compressed at quality 11.
 * Returns the total in bytes and the files, in the order index.html names them. xtask reports it
 * in KB of 1000 bytes, as Vite's own `kB` output does. Throws when there is no module script.
 */
export function initialJs(dist) {
  const html = readFileSync(join(dist, "index.html"), "utf8");
  const files = [];
  let entries = 0;
  for (const tag of html.match(/<script\b[^>]*>|<link\b[^>]*>/g) ?? []) {
    const isEntry = tag.startsWith("<script") && /\btype="module"/.test(tag);
    const isPreload = tag.startsWith("<link") && /\brel="modulepreload"/.test(tag);
    const url = /\b(?:src|href)="([^"]+)"/.exec(tag)?.[1];
    if ((isEntry || isPreload) && url !== undefined) {
      files.push(url.replace(/^\//, ""));
      entries += isEntry ? 1 : 0;
    }
  }
  if (entries === 0) {
    throw new Error("dist/index.html has no module script");
  }
  let bytes = 0;
  for (const file of files) {
    const compressed = brotliCompressSync(readFileSync(join(dist, file)), {
      params: { [zlib.BROTLI_PARAM_QUALITY]: 11 },
    });
    bytes += compressed.length;
  }
  return { bytes, files };
}
