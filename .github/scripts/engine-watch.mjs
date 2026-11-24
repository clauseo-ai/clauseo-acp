// Engine watch: bumps G16's pins (crates/limits/tests/live/package.json and its lock) when npm
// publishes a newer Claude or Codex adapter, and re-measures what the app pins from them.
// engine-watch.yml runs it every 6 hours; engine-watch.test.mjs tests it. Plain Node 24, no install.
// The building blocks are in engine-watch-lib.mjs and engine-watch-sac.mjs, re-exported here.
//
//   node .github/scripts/engine-watch.mjs [--root <dir>] [--summary <file>] [--min-age-hours <n>]
//   node .github/scripts/engine-watch.mjs --check-sizes [--root <dir>]
//   node .github/scripts/engine-watch.mjs --push-plan <branch> <git ls-remote output>
//
// <dir> is the repository root (default: this checkout); only the pins pair, engines.seq,
// crates/agents/src/snapshot/embedded.rs and crates/agents/src/sac.rs below it are read or
// written, so a scratch copy of those files with the same layout works too.
//
// Which versions: npm's `latest` of claude-agent-acp, codex-acp and @openai/codex, each taken only
// when newer than its pin, a release (no pre-release part), not deprecated, and published at least
// the cool-down ago (`time[version]`): 24 hours on a scheduled run; by hand MIN_AGE_HOURS (the
// dispatch input min_age_hours, a non-negative integer, default 24) or --min-age-hours. A version
// that fails stays at its pin and the summary says why; the others can still bump. The codex
// override goes to latest @openai/codex, or to the newest release in codex-acp's own range when
// that range excludes it. A codex side that cannot go together (codex-acp's range excluding the
// codex) holds codex-acp and codex at their pins; a proposed pair outside the range throws.
//
// The bump: package.json rewritten, the lock regenerated with
// `npm install --package-lock-only --ignore-scripts --no-audit --no-fund` in its folder, SIZES and
// SIZES_STAMP re-measured (engine-watch-lib.mjs), sac.rs's LAYOUTS re-hashed when the codex
// changes (engine-watch-sac.mjs), engines.seq set to max(its value, the published seq) + 1, and a
// markdown summary written to <file>: versions, ages, whether each of the four top-level packages
// has npm provenance attestations (recorded, not verified). With nothing to take it prints
// `up to date`. Under GitHub Actions it writes `bumped` and `branch` to $GITHUB_OUTPUT; every
// version in them is rebuilt from its parsed parts.
//
// --push-plan never overwrites: a branch that exists already (any tree) means no push. It writes
// `push` (true or false) to $GITHUB_OUTPUT.
//
// Exit status: 0 when up to date, bumped, planned or the sizes match; 1 on a mismatch or failure.

import { spawnSync } from "node:child_process";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  ARCH_NPM, CLAUDE_ACP, CODEX, CODEX_ACP, EMBEDDED, LIVE, OS_NPM, SDK, SEQ_FILE,
  cleanVersion, compareVersions, docUrl, fetchJson, maxSatisfying, measureSizes, nextSeq,
  parseSeq, parseSizes, parseVersion, satisfies, shortHash, writeSizes,
} from "./engine-watch-lib.mjs";
import { SAC, hashLayouts, parseLayouts, writeLayouts } from "./engine-watch-sac.mjs";

export * from "./engine-watch-lib.mjs";
export * from "./engine-watch-sac.mjs";

const HOUR = 3_600_000;
export const BRANCH_RE = /^engines\/claude-acp-\d+\.\d+\.\d+_codex-acp-\d+\.\d+\.\d+_codex-\d+\.\d+\.\d+$/;

/** The cool-down in hours: 24 on a schedule; otherwise `input` (default 24), a whole number. */
export function minAgeHours(event, input) {
  if (event === "schedule") return 24;
  const text = String(input ?? "").trim();
  if (text === "") return 24;
  if (!/^\d+$/.test(text)) throw new Error(`min_age_hours must be a non-negative integer, not ${JSON.stringify(input)}`);
  return Number(text);
}

/** Registry text made safe for one markdown line. */
const plain = (text) => String(text).replace(/[^\w .,:;()/@'+^~<>=|*-]/g, " ").replace(/\s+/g, " ").trim().slice(0, 200);

/** Whether `raw` (from `packument`) may be pinned: { version, ok, ageHours?, reason? }. */
export function judge(packument, raw, { now, minAge }) {
  let version;
  try {
    version = cleanVersion(raw);
  } catch {
    return { version: plain(raw), ok: false, reason: "not a version" };
  }
  if (parseVersion(version).pre.length) return { version, ok: false, reason: "a pre-release" };
  const doc = packument.versions?.[version];
  if (!doc) return { version, ok: false, reason: "not among the registry's versions" };
  if (doc.deprecated !== undefined) return { version, ok: false, reason: `deprecated: ${plain(doc.deprecated)}` };
  const published = Date.parse(packument.time?.[version] ?? "");
  if (Number.isNaN(published)) return { version, ok: false, reason: "no publish time in the registry" };
  const ageHours = Math.floor((now - published) / HOUR);
  if (now - published < minAge * HOUR) {
    return { version, ok: false, ageHours, reason: `${ageHours} h old, under the ${minAge} h cool-down` };
  }
  return { version, ok: true, ageHours };
}

/** codex-acp `version`'s own @openai/codex range, or undefined. */
const codexRange = (pk, version) => pk[CODEX_ACP].versions?.[version]?.dependencies?.[CODEX];

/** Throws unless a changed codex side is one codex-acp's own range allows. */
export function assertPair(current, target, pk) {
  if (target.codexAcp === current.codexAcp && target.codex === current.codex) return;
  const range = codexRange(pk, target.codexAcp);
  if (range && !satisfies(target.codex, range)) {
    throw new Error(`codex-acp ${target.codexAcp} asks for @openai/codex ${range}, not ${target.codex}: not proposing that pair`);
  }
}

/**
 * The versions to pin, from the three packuments `pk` (by name), and the notes for the summary.
 * Returns { target, changed, notes, verdicts }.
 */
export function planBump(current, pk, opts) {
  const notes = [];
  const verdicts = {};
  const take = (key, name, raw, cur) => {
    const verdict = judge(pk[name], raw, opts);
    verdicts[key] = verdict;
    const newer = !parseVersion(verdict.version) || compareVersions(verdict.version, cur) > 0;
    if (verdict.ok && newer) return verdict.version;
    if (!verdict.ok && newer) notes.push(`${name} ${verdict.version} not taken, kept at ${cur}: ${verdict.reason}.`);
    return cur;
  };
  const latest = (name) => pk[name]["dist-tags"]?.latest;
  const target = {
    claudeAcp: take("claudeAcp", CLAUDE_ACP, latest(CLAUDE_ACP), current.claudeAcp),
    codexAcp: take("codexAcp", CODEX_ACP, latest(CODEX_ACP), current.codexAcp),
  };
  const range = codexRange(pk, target.codexAcp);
  let candidate = latest(CODEX);
  if (!range) {
    notes.push(`codex-acp ${target.codexAcp} declares no @openai/codex range: the override is checked against none.`);
  } else if (parseVersion(String(candidate)) && !satisfies(cleanVersion(candidate), range)) {
    const own = maxSatisfying(Object.keys(pk[CODEX].versions ?? {}), range);
    notes.push(`@openai/codex ${plain(candidate)} is npm's latest, but codex-acp ${target.codexAcp} asks for \`${plain(range)}\`: ${own ? `the newest release in that range is ${own}` : "no release is in that range"}.`);
    candidate = own ?? current.codex;
  }
  target.codex = take("codex", CODEX, candidate, current.codex);
  if (target.codexAcp !== current.codexAcp && range && !satisfies(target.codex, range)) {
    notes.push(`codex-acp ${target.codexAcp} not taken, kept at ${current.codexAcp} with @openai/codex ${current.codex}: it needs a codex in \`${plain(range)}\` that is not ready.`);
    target.codexAcp = current.codexAcp;
    target.codex = current.codex;
  }
  assertPair(current, target, pk);
  const changed = Object.keys(target).some((key) => target[key] !== current[key]);
  return { target, changed, notes, verdicts };
}

/** The pinned versions in package.json, each rebuilt from its parts. */
export function currentPins(manifest) {
  return {
    claudeAcp: cleanVersion(manifest.dependencies[CLAUDE_ACP]),
    codexAcp: cleanVersion(manifest.dependencies[CODEX_ACP]),
    codex: cleanVersion(manifest.overrides[CODEX]),
  };
}

export function branchName(t) {
  const name = `engines/claude-acp-${cleanVersion(t.claudeAcp)}_codex-acp-${cleanVersion(t.codexAcp)}_codex-${cleanVersion(t.codex)}`;
  if (!BRANCH_RE.test(name)) throw new Error(`not a branch this job makes: ${JSON.stringify(name)}`);
  return name;
}

/** Whether to push `branch`, from `git ls-remote origin refs/heads/<branch>`: never over one. */
export function pushPlan(branch, lsRemote) {
  if (!BRANCH_RE.test(branch)) throw new Error(`not a branch this job makes: ${JSON.stringify(branch)}`);
  const sha = String(lsRemote ?? "").trim().split(/\s+/)[0];
  if (sha) {
    return { push: false, message: `::notice::${branch} exists already (${sha.slice(0, 12)}): nothing pushed. Merge or delete it, and the next run pushes again.` };
  }
  return { push: true, message: `${branch} is new: pushing it.` };
}

/** package.json with the target versions, written as npm writes it. */
export function bumpManifest(manifest, target) {
  const next = structuredClone(manifest);
  next.dependencies[CLAUDE_ACP] = target.claudeAcp;
  next.dependencies[CODEX_ACP] = target.codexAcp;
  next.overrides[CODEX] = target.codex;
  return `${JSON.stringify(next, null, 2)}\n`;
}

/** Whether npm lists provenance attestations for `version` in `packument`. */
export const attested = (packument, version) => Boolean(packument?.versions?.[version]?.dist?.attestations);

/** One provenance row; `flag` when a new version lacks what the old pin had. */
export function provenance(name, packument, oldVersion, newVersion) {
  const had = attested(packument, oldVersion);
  const has = attested(packument, newVersion);
  return { name, old: oldVersion, new: newVersion, had, has, flag: oldVersion !== newVersion && had && !has };
}

/** The markdown summary: the commit message body and the job summary. */
export function summary(s) {
  const { current, target, ages, sdk, claudeCode, notes, prov, rows, oldRows, stamp, seq, layouts } = s;
  const mb = (n) => `${(n / 1e6).toFixed(1)} MB`;
  const yes = (b) => (b ? "yes" : "no");
  const age = (h) => (h === undefined || h === null ? "?" : `${h} h`);
  const line = (name, a, b, hours, p) =>
    `| \`${name}\`${name === CODEX ? " (override)" : ""} | ${a} | ${b}${a === b ? " (unchanged)" : ""} | ${a === b ? "" : age(hours)} | ${yes(p.had)} → ${yes(p.has)}${p.flag ? " **lost**" : ""} |`;
  const lines = [
    "Engine bump from engine-watch: npm published newer adapters than G16's pins",
    "(`crates/limits/tests/live/package.json` and its lock).",
    "",
    "| Package | Pinned | New | Age | Provenance attestations |",
    "|---|---|---|---|---|",
    line(CLAUDE_ACP, current.claudeAcp, target.claudeAcp, ages.claudeAcp, prov[0]),
    line(SDK, sdk.old, sdk.new, ages.sdk, prov[1]),
    line(CODEX_ACP, current.codexAcp, target.codexAcp, ages.codexAcp, prov[2]),
    line(CODEX, current.codex, target.codex, ages.codex, prov[3]),
    "",
    "npm signatures and provenance were not verified.",
    ...prov.filter((p) => p.flag).map((p) => `- **${p.name} ${p.new} has no provenance attestations; ${p.old} had them.**`),
    `Claude Code inside the SDK (its \`claudeCodeVersion\`): ${claudeCode.old ?? "?"} → ${claudeCode.new ?? "?"}.`,
    "Model ids: not listed in the packages' registry metadata, so not compared here.",
    `Engine seq (\`${LIVE}/${SEQ_FILE}\`): ${seq.old} → ${seq.new}.`,
  ];
  if (notes.length) lines.push("", ...notes.map((n) => `- ${n}`));
  lines.push("", `SIZES re-measured from the registry (SIZES_STAMP ${stamp}):`, "", "| Agent | Platform | Download | Unpacked |", "|---|---|---|---|");
  rows.forEach((r, i) => {
    const old = oldRows[i];
    lines.push(`| ${r.agent} | ${OS_NPM[r.os]}-${ARCH_NPM[r.arch]} | ${mb(old.download)} → ${mb(r.download)} | ${mb(old.unpacked)} → ${mb(r.unpacked)} |`);
  });
  lines.push("", "Before merging:", "- the suite and a real-agent smoke on the CI thread's lanes (nothing runs on GitHub);");
  if (layouts.rehashed) {
    lines.push(
      layouts.changed
        ? `- rg.exe re-hashed in \`${SAC}\` LAYOUTS for @openai/codex ${target.codex}, and it changed: check it is still the official ripgrep release the doc comment names;`
        : `- rg.exe re-hashed in \`${SAC}\` LAYOUTS for @openai/codex ${target.codex}: unchanged, LAYOUTS_CODEX moved;`,
    );
  }
  if (target.codexAcp !== current.codexAcp) {
    lines.push("- `crates/agents/src/install/codex_patch.rs` (VERSION, PRISTINE, PATCHED) redone for the new codex-acp `dist/index.js`;");
  }
  lines.push("- the lock's entry counts in `crates/agents/src/snapshot/tests.rs` checked.");
  return `${lines.join("\n")}\n`;
}

function versionIn(lockText, name) {
  return JSON.parse(lockText).packages[`node_modules/${name}`]?.version ?? null;
}

function setOutput(pairs) {
  const file = process.env.GITHUB_OUTPUT;
  if (!file) return;
  for (const [k, v] of Object.entries(pairs)) if (/[\r\n]/.test(String(v))) throw new Error(`output ${k} has a line break`);
  appendFileSync(file, Object.entries(pairs).map(([k, v]) => `${k}=${v}\n`).join(""));
}

function defaultNpm(dir) {
  const npm = spawnSync("npm", ["install", "--package-lock-only", "--ignore-scripts", "--no-audit", "--no-fund"], {
    cwd: dir,
    stdio: "inherit",
    shell: process.platform === "win32",
  });
  if (npm.status !== 0) throw new Error(`npm install --package-lock-only failed (${npm.status ?? npm.signal})`);
}

export async function checkSizes(root, { fetchImpl = fetch, log = console.log } = {}) {
  const lockText = readFileSync(join(root, LIVE, "package-lock.json"), "utf8");
  const { rows, stamp } = parseSizes(readFileSync(join(root, EMBEDDED), "utf8"));
  const measured = await measureSizes(lockText, rows, fetchImpl);
  const hash = shortHash([lockText]);
  let ok = hash === stamp;
  log(`SIZES_STAMP ${stamp}, lock hash ${hash}${ok ? "" : " (differs)"}`);
  measured.forEach((m, i) => {
    const r = rows[i];
    const same = m.download === r.download && m.unpacked === r.unpacked;
    ok &&= same;
    log(`${same ? "match" : "DIFF "} ${m.agent} ${m.os} ${m.arch}: ${m.download} ${m.unpacked}${same ? "" : ` (embedded ${r.download} ${r.unpacked})`}`);
  });
  log(ok ? "SIZES match the lock" : "SIZES do not match the lock");
  return ok ? 0 : 1;
}

/** The whole bump; returns { code, branch? }. `fetchImpl`, `runNpm` and `now` are for tests. */
export async function bump(root, { summaryPath, minAge = 24, fetchImpl = fetch, runNpm = defaultNpm, now = Date.now(), log = console.log } = {}) {
  const dir = join(root, LIVE);
  const file = (rel) => join(root, rel);
  const manifestPath = join(dir, "package.json");
  const lockPath = join(dir, "package-lock.json");
  const seqPath = join(dir, SEQ_FILE);
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const oldLock = readFileSync(lockPath, "utf8");
  const current = currentPins(manifest);
  const names = [CLAUDE_ACP, CODEX_ACP, CODEX];
  const docs = await Promise.all(names.map((name) => fetchJson(fetchImpl, docUrl(name))));
  const pk = Object.fromEntries(names.map((name, i) => [name, docs[i]]));
  const { target, changed, notes, verdicts } = planBump(current, pk, { now, minAge });
  if (!changed) {
    log("up to date");
    notes.forEach((n) => log(n));
    setOutput({ bumped: "false" });
    return { code: 0 };
  }
  const branch = branchName(target);
  const seqText = readFileSync(seqPath, "utf8");
  const seq = { old: parseSeq(seqText), new: await nextSeq(seqText, fetchImpl) };
  writeFileSync(manifestPath, bumpManifest(manifest, target));
  writeFileSync(seqPath, `${seq.new}\n`);
  runNpm(dir);
  const lockText = readFileSync(lockPath, "utf8");

  const embedded = readFileSync(file(EMBEDDED), "utf8");
  const { rows: oldRows } = parseSizes(embedded);
  const rows = await measureSizes(lockText, oldRows, fetchImpl);
  const stamp = shortHash([lockText]);
  writeFileSync(file(EMBEDDED), writeSizes(embedded, rows, stamp));

  const sac = readFileSync(file(SAC), "utf8");
  const oldLayouts = parseLayouts(sac);
  const layouts = { rehashed: false, changed: false };
  if (oldLayouts.codex !== target.codex) {
    const download = async (url) => Buffer.from(await (await fetchImpl(url)).arrayBuffer());
    const hashed = await hashLayouts(lockText, oldLayouts.rows, download);
    layouts.rehashed = true;
    layouts.changed = hashed.some((row, i) => row.sha256 !== oldLayouts.rows[i].sha256);
    writeFileSync(file(SAC), writeLayouts(sac, hashed, target.codex));
  }

  const sdkDoc = await fetchJson(fetchImpl, docUrl(SDK));
  const sdk = { old: versionIn(oldLock, SDK), new: versionIn(lockText, SDK) };
  const sdkTime = Date.parse(sdkDoc.time?.[sdk.new] ?? "");
  const ages = {
    claudeAcp: verdicts.claudeAcp.ageHours,
    codexAcp: verdicts.codexAcp.ageHours,
    codex: verdicts.codex.ageHours,
    sdk: Number.isNaN(sdkTime) ? null : Math.floor((now - sdkTime) / HOUR),
  };
  const claudeCode = { old: sdkDoc.versions?.[sdk.old]?.claudeCodeVersion, new: sdkDoc.versions?.[sdk.new]?.claudeCodeVersion };
  const prov = [
    provenance(CLAUDE_ACP, pk[CLAUDE_ACP], current.claudeAcp, target.claudeAcp),
    provenance(SDK, sdkDoc, sdk.old, sdk.new),
    provenance(CODEX_ACP, pk[CODEX_ACP], current.codexAcp, target.codexAcp),
    provenance(CODEX, pk[CODEX], current.codex, target.codex),
  ];
  const text = summary({ current, target, ages, sdk, claudeCode, notes, prov, rows, oldRows, stamp, seq, layouts });
  if (summaryPath) writeFileSync(summaryPath, text);
  log(text);
  log(`bump ${branch}`);
  setOutput({ bumped: "true", branch });
  return { code: 0, branch };
}

export async function main(argv, env = process.env) {
  const arg = (flag) => {
    const i = argv.indexOf(flag);
    return i === -1 ? undefined : argv[i + 1];
  };
  const root = resolve(arg("--root") ?? join(dirname(fileURLToPath(import.meta.url)), "../.."));
  if (argv.includes("--check-sizes")) return checkSizes(root);
  if (argv.includes("--push-plan")) {
    const i = argv.indexOf("--push-plan");
    const plan = pushPlan(argv[i + 1], argv[i + 2]);
    console.log(plan.message);
    setOutput({ push: String(plan.push) });
    return 0;
  }
  const minAge = minAgeHours(env.GITHUB_EVENT_NAME, arg("--min-age-hours") ?? env.MIN_AGE_HOURS);
  return (await bump(root, { summaryPath: arg("--summary"), minAge })).code;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (err) => {
      console.error(`engine-watch: ${err.message ?? err}`);
      process.exit(1);
    },
  );
}
