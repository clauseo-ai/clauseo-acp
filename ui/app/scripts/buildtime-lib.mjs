// The pure parts of buildtime.mjs, kept apart so buildtime-lib.test.mjs can test them: argument
// parsing, the commands from package.json, and sizing the initial JS of a build.

import { existsSync, readFileSync } from "node:fs";
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

/** Characters that make a package.json script more than a plain command and its arguments. */
const SHELL_SYNTAX = /[&|;<>()$`'"\\*?{}[\]~#]/;

/**
 * The words of the package.json script `text`, split on spaces. Throws on shell syntax (`&&`,
 * `|`, quotes, `$`, globs, `VAR=value`): the script runs it without a shell, so it must be one
 * plain command.
 */
export function splitCommand(name, text) {
  if (typeof text !== "string" || text.trim() === "") {
    throw new Error(`ui/app/package.json has no script "${name}"`);
  }
  if (SHELL_SYNTAX.test(text) || text.trim().split(/ +/)[0].includes("=")) {
    throw new Error(
      `ui/app/package.json script "${name}" is \`${text}\`; buildtime runs scripts without a shell, so keep it one plain command`,
    );
  }
  return text.trim().split(/ +/);
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

/**
 * The JS entry of the bin `name`: the first direct dependency, in `dirs` order, whose
 * package.json `bin` provides it, read from `<dir>/node_modules/<dependency>`. A string `bin`
 * provides the package's own name without its scope. Throws when no dependency provides it.
 */
export function resolveBin(name, dirs) {
  for (const dir of dirs) {
    const manifest = readJson(join(dir, "package.json"));
    const deps = Object.keys({ ...manifest.dependencies, ...manifest.devDependencies });
    for (const dep of deps) {
      const pkgDir = join(dir, "node_modules", dep);
      if (!existsSync(join(pkgDir, "package.json"))) {
        continue;
      }
      const pkg = readJson(join(pkgDir, "package.json"));
      const own = String(pkg.name ?? dep).replace(/^@[^/]+\//, "");
      const entry =
        typeof pkg.bin === "string" ? (own === name ? pkg.bin : undefined) : pkg.bin?.[name];
      if (typeof entry === "string") {
        return join(pkgDir, entry);
      }
    }
  }
  throw new Error(
    `no dependency in ${dirs.map((dir) => join(dir, "package.json")).join(" or ")} provides the bin "${name}"; run \`pnpm -C ui install --frozen-lockfile\``,
  );
}

/**
 * The package.json script `script` of the app in `appDir`, ready to run without a shell or pnpm:
 * `argv` is the bin's JS entry and the script's arguments, for `process.execPath`. Bins are looked
 * up in the app, then in the ui workspace root (`rootDir`).
 */
export function scriptCommand(appDir, rootDir, script) {
  const scripts = readJson(join(appDir, "package.json")).scripts ?? {};
  const [bin, ...args] = splitCommand(script, scripts[script]);
  return { argv: [resolveBin(bin, [appDir, rootDir]), ...args], text: scripts[script] };
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
