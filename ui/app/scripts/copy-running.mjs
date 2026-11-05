// The running copy review page: `pnpm -C ui/app copy:running` writes copy-review/running/index.html (beside the repo:
// ../copy-review/ from its root), listing every provisional entry in ui/app/copy-approvals/, grouped by file: its id,
// English, where it shows (from the review registry, if it has the id), when it was added and by whom, and, where the
// review gallery can render the id, a shot of its cell by day and by night. A line the gallery has no cell for is
// listed with "no shot". Rohan reviews these lines in batches, then marks them approved. Under the lines, a Docs section
// lists the provisional docs in ui/app/doc-approvals.json (--docs <file> points it elsewhere; a missing file means
// none): each doc's title from its first "# " heading, a link to its last commit on GitHub, and who wrote it in which
// pull request.
//
// The whole page is built in running.next/ (beside running/) and swapped in with two renames once every step has
// succeeded, so a browser never sees a half-built folder, and a failed run leaves the old page as it was.
//
// The gallery is built into review/ in the staging folder (so each line also links to its live cell) and shot from
// there with agent-browser, headless, in a session of its own, closed after. --approvals <dir>, --out <dir> and
// --index <file> (the index to add the page's line to, once; default: the index beside --out) point it elsewhere.
import { execFile, execFileSync } from "node:child_process";
import {
  createReadStream,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createServer as createHttp } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { build, createServer } from "vite";
import { docsSection, esc } from "./running-docs.mjs";

const app = fileURLToPath(new URL("..", import.meta.url));
const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(name);
  return i === -1 ? fallback : args[i + 1];
};
const approvalsDir = opt("--approvals", join(app, "copy-approvals"));
const reviewRoot = join(app, "..", "..", "..", "copy-review");
const out = opt("--out", join(reviewRoot, "running"));
const docsFile = opt("--docs", join(app, "doc-approvals.json"));
const repoRoot = join(app, "..", "..");
const next = `${out}.next`;
const old = `${out}.old`;
const configFile = join(app, "vite.review.config.ts");
const DESKTOP = { width: 1100, height: 600 };

// The approvals' checks and the registry, as the gallery and the gate have them, loaded through Vite.
const vite = await createServer({
  configFile,
  root: app,
  logLevel: "error",
  appType: "custom",
  server: { middlewareMode: true, hmr: false, ws: false },
});
let mergeApprovals, parseApprovals, REGISTRY;
try {
  ({ mergeApprovals, parseApprovals } = await vite.ssrLoadModule("/src/review/status.ts"));
  ({ REGISTRY } = await vite.ssrLoadModule("/src/review/registry.tsx"));
} finally {
  await vite.close();
}

// Every file, parsed strictly; the merge refuses an id in two files.
const names = existsSync(approvalsDir)
  ? readdirSync(approvalsDir)
      .filter((f) => f.endsWith(".json"))
      .sort()
  : [];
const parsed = Object.fromEntries(
  names.map((f) => [f, JSON.parse(readFileSync(join(approvalsDir, f), "utf8"))]),
);
mergeApprovals(parsed);
const groups = names
  .map((file) => ({
    file,
    lines: Object.entries(parseApprovals(parsed[file], file))
      .filter(([, a]) => a.status === "provisional")
      .map(([id, a]) => ({ id, ...a, entry: REGISTRY.find((e) => e.id === id) })),
  }))
  .filter((g) => g.lines.length > 0);
const lines = groups.flatMap((g) => g.lines);

// The docs: each provisional entry of doc-approvals.json, its title from its first "# " heading and its last commit.
const docEntries = existsSync(docsFile)
  ? Object.entries(JSON.parse(readFileSync(docsFile, "utf8")))
  : [];
const docs = docEntries
  .filter(([, d]) => d.status === "provisional")
  .map(([path, d]) => {
    const heading = readFileSync(join(repoRoot, path), "utf8")
      .split("\n")
      .find((l) => l.startsWith("# "));
    if (heading === undefined) throw new Error(`${path} has no "# " heading for its title`);
    const commit = execFileSync("git", ["-C", repoRoot, "log", "-1", "--format=%H", "--", path], {
      encoding: "utf8",
    }).trim();
    if (commit === "") throw new Error(`${path} has no commit in git`);
    return { path, title: heading.slice(2).trim(), commit, by: d.by, pr: d.pr };
  });

// The count line, once for the page and the log: the lines, then the docs, with the zero-and-zero text as it was.
function countLine(lineGroups, docCount) {
  const docText = `${docCount} provisional ${docCount === 1 ? "doc" : "docs"}`;
  if (lines.length === 0) {
    return docCount === 0
      ? "No provisional lines: nothing is new or changed since Rohan's last review."
      : `No provisional lines; ${docText} below.`;
  }
  const base = `${lines.length} provisional ${lines.length === 1 ? "line" : "lines"} in ${lineGroups.length} ${lineGroups.length === 1 ? "file" : "files"}: ${lines.map((l) => l.id).join(", ")}`;
  return docCount === 0 ? `${base}.` : `${base} and ${docText}.`;
}
const count = countLine(groups, docs.length);

// Everything is written into the staging folder; the swap comes last, and only if every step has succeeded.
const reviewOut = join(next, "review");
const shot = new Map();
rmSync(next, { recursive: true, force: true });
let swapped = false;
try {
  mkdirSync(next, { recursive: true });
  // The gallery: its stylesheet styles the page, its cells are shot and linked.
  await build({
    configFile,
    root: app,
    logLevel: "error",
    build: { outDir: reviewOut, emptyOutDir: true },
  });
  const css = readdirSync(join(reviewOut, "assets")).find((f) => /^review-.*\.css$/.test(f));
  if (css === undefined) throw new Error("the review build has no review-*.css");

  // Shots: each line the registry has, by day and by night, in English, at its window's size, at 2x.
  const shotsDir = join(next, "shots");
  mkdirSync(shotsDir, { recursive: true });
  const shootable = lines.filter((l) => l.entry !== undefined);
  if (shootable.length > 0) {
    const TYPES = {
      ".html": "text/html",
      ".js": "text/javascript",
      ".css": "text/css",
      ".woff2": "font/woff2",
      ".svg": "image/svg+xml",
      ".png": "image/png",
    };
    const http = createHttp((req, res) => {
      const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname)).replace(
        /^(\.\.[/\\])+/,
        "",
      );
      const file = join(reviewOut, path);
      if (!file.startsWith(reviewOut) || !existsSync(file) || !statSync(file).isFile()) {
        res.writeHead(404).end();
        return;
      }
      res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
      createReadStream(file).pipe(res);
    });
    await new Promise((done) => http.listen(0, "127.0.0.1", done));
    const port = http.address().port;
    const session = `copy-running-${process.pid}`;
    // asynchronous, so this process's server answers the browser while agent-browser waits
    const ab = (...a) => promisify(execFile)("agent-browser", ["--session", session, ...a]);
    try {
      await ab("open", "about:blank");
      for (const line of shootable) {
        const width = line.entry.width ?? DESKTOP.width;
        const height = line.entry.height ?? DESKTOP.height;
        await ab("set", "viewport", String(width), String(height), "2");
        for (const theme of ["day", "night"]) {
          const q = new URLSearchParams({ cell: line.id, lang: "en", theme, platform: "mac" });
          await ab("open", `http://127.0.0.1:${port}/review.html?${q}`);
          // the state fills in as its scripted host answers; the review marks the string once it shows
          try {
            await ab("wait", "--fn", "document.querySelector('[data-review-mark]') !== null");
          } catch {
            // no mark (a string only in a tooltip, say): shoot the state as it stands
          }
          await ab("wait", "500");
          const name = `${line.id}-${theme}.png`;
          await ab("screenshot", join(shotsDir, name));
          shot.set(`${line.id}-${theme}`, `shots/${name}`);
        }
      }
    } finally {
      try {
        await ab("close");
      } catch {
        // already gone
      }
      http.close();
    }
  }

  // The page, styled as the archived-read-only page is.
  const at = new Date();
  const stamp = `${at.toLocaleString("en-GB", { dateStyle: "long", timeStyle: "short" })} (${at.toISOString()})`;
  const PAGE_CSS = `/* The running page's own lines, beside the review app's sheet (review-*.css), as the archived-read-only page has them. */
.review-shot { display: block; width: 100%; height: auto; }
.review-note { color: var(--muted); font-size: 0.8125rem; margin: 4px 0 0; }
.running-file { font-size: 1.125rem; margin: 32px 0 0; font-family: ui-monospace, monospace; }
.running-words { font-size: 1rem; font-weight: 600; margin: 4px 0 6px; }
.running-where { max-width: 60rem; margin: 4px 0 6px; }
.running-status { margin: 0 0 6px; }
.running-hi, .running-screen { color: var(--muted); }
.running-screen { font-size: 0.875rem; margin: 0 0 6px; }
.running-live { margin: 0 0 12px; font-size: 0.875rem; }
.running-shots { display: flex; flex-wrap: wrap; gap: 16px; align-items: flex-start; }
.running-shot { margin: 0; width: 300px; max-width: 100%; }
.running-shot figcaption { font-size: 0.8125rem; color: var(--muted); margin-bottom: 4px; }
.running-noshot { color: var(--muted); font-size: 0.875rem; margin: 0; }
`;
  mkdirSync(join(next, "assets"), { recursive: true });
  writeFileSync(join(next, "assets", "page.css"), PAGE_CSS);

  const item = (l) => {
    const where =
      l.entry === undefined
        ? "Not in the review registry."
        : `${l.entry.screen}, ${l.entry.width ?? DESKTOP.width} × ${l.entry.height ?? DESKTOP.height}`;
    const words =
      l.entry === undefined
        ? ""
        : `<code class="review-where">${esc(l.entry.words.file)} : ${esc(JSON.stringify(l.entry.words.key))}</code>`;
    const live =
      l.entry === undefined
        ? ""
        : `<p class="running-live">Live cell: ${["day", "night"]
            .map(
              (t) =>
                `<a href="review/review.html?${esc(new URLSearchParams({ cell: l.id, theme: t, platform: "mac" }).toString())}">${t}</a>`,
            )
            .join(" · ")}</p>`;
    const shots = ["day", "night"]
      .filter((t) => shot.has(`${l.id}-${t}`))
      .map((t) => {
        const src = shot.get(`${l.id}-${t}`);
        return `<figure class="running-shot"><figcaption>${t}</figcaption><div class="review-box"><a href="${src}"><img class="review-shot" src="${src}" alt="${esc(`${l.id}, ${l.en}, ${t}`)}" loading="lazy"></a></div></figure>`;
      });
    return `<article class="review-item" id="${esc(l.id)}"><header class="review-item-head"><h2>${esc(l.id)}</h2><span class="review-status" data-status="provisional">Provisional</span>${words}</header>
<p class="running-words" lang="en">${esc(l.en)}</p>
<p class="running-status">Added ${esc(l.at)} by ${esc(l.by)}. <span class="running-hi">Hindi: ${l.hi === null ? "later" : `<span lang="hi">${esc(l.hi)}</span>`}.</span>${l.note === undefined ? "" : ` ${esc(l.note)}`}</p>
<p class="running-screen">Shown in: ${esc(where)}</p>
${live}
${shots.length === 0 ? `<p class="running-noshot">no shot</p>` : `<div class="running-shots">${shots.join("")}</div>`}
</article>`;
  };

  const html = `<!doctype html>
<html lang="en" data-theme="day" data-render="full" data-motion="full" data-text-size="16" data-platform="mac"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Clauseo copy review: new or changed since Rohan's last review</title>
<link rel="stylesheet" href="review/assets/${css}"><link rel="stylesheet" href="assets/page.css">
</head>
<body>
<main class="review-page">
  <header class="review-head">
    <h1>New or changed since Rohan's last review</h1>
    <p class="review-count">${esc(count)}</p>
    <p class="review-note">Generated ${esc(stamp)} by <code>pnpm -C ui/app copy:running</code>, from ui/app/copy-approvals/ and ui/app/doc-approvals.json.</p>
    <p class="review-note">Each line here is "status": "provisional": its words follow DESIGN.md and the word list and went in without waiting for a yes. Approve or ask for a change by id. The shots show the line's cell in the review gallery at its window's size, day and night, at 2×, in English; a line the gallery has no cell for says "no shot".</p>
  </header>
${groups.map((g) => `<h2 class="running-file">${esc(g.file)}</h2>\n${g.lines.map(item).join("\n")}`).join("\n")}
${docsSection(docs)}
</main>
</body></html>
`;
  writeFileSync(join(next, "index.html"), html);

  // The swap: two renames in out's parent. If the second fails, the first is put back.
  if (existsSync(out)) {
    rmSync(old, { recursive: true, force: true });
    renameSync(out, old);
  }
  try {
    renameSync(next, out);
  } catch (e) {
    if (existsSync(old)) renameSync(old, out);
    throw e;
  }
  swapped = true;
  rmSync(old, { recursive: true, force: true });
} finally {
  if (!swapped) rmSync(next, { recursive: true, force: true });
}

// One line for the page in the copy review's index, inserted once: the index beside out, not beside the staging folder.
const index = opt("--index", join(dirname(out), "index.html"));
if (existsSync(index)) {
  const text = readFileSync(index, "utf8");
  if (!text.includes('href="running/')) {
    const li = `<li><a href="running/index.html">running</a> (the running page: every provisional line, words that follow DESIGN.md and went in without waiting for your yes, grouped by brief, with shots day and night where the gallery has the cell. Regenerated by <code>pnpm -C ui/app copy:running</code>)</li>\n`;
    const at = text.indexOf("<ul>\n");
    if (at === -1) throw new Error(`${index} has no <ul> to add the running page to`);
    writeFileSync(index, text.slice(0, at + 5) + li + text.slice(at + 5));
  }
}

console.log(`${join(out, "index.html")}: ${count} ${shot.size} shots.`);
