// agent-browser-build.mjs: our build of agent-browser, upstream's source at the commit
// crates/desktop/agent-browser.toml pins plus the patches in crates/desktop/agent-browser/patches/
// (crates/desktop/agent-browser/README.md says what they do and how to bump them).
//
//   node crates/desktop/sidecars/agent-browser-build.mjs check
//   node crates/desktop/sidecars/agent-browser-build.mjs build --target <triple> --out <dir>
//   node crates/desktop/sidecars/agent-browser-build.mjs tag
//   node crates/desktop/sidecars/agent-browser-build.mjs toolchain
//
// `check` downloads GitHub's source archive of `upstream_commit`, refuses it unless its SHA-256 is
// `upstream_tarball_sha256` and the archive names that commit, extracts it to a temporary folder,
// and applies every patch in order with `git apply`, after a `git apply --check` of each. It runs
// on any machine with node, tar and git: it is the check that the patches apply cleanly to the
// pinned tag. The temporary folder is removed whatever happens.
//
// `build` does the same, then runs the patched source's own test suite for one of the six
// release triples (`cargo test --locked --target <triple>`, as upstream's CI runs it; on Windows
// this is where the pipe's restricted-token test and the hardening tests run), builds it with
// `cargo build --release --locked --target <triple>` in its cli/ folder, and writes
// `<dir>/agent-browser-<triple>[.exe]` and `<dir>/agent-browser-<triple>.json`, the build record:
// the triple, the exe's SHA-256 and size, `rustc -vV`, the MSVC tools and linker versions where
// they can be found, the upstream commit, the archive's and each patch's SHA-256. It prints the
// pin's two lines for that triple. .github/workflows/agent-browser-build.yml runs it on a runner
// per triple (native, except x64 macOS, cross-compiled on an ARM64 Mac).
//
// `build` refuses to run when `git status` shows uncommitted changes in the patches folder, and
// when the toolchain's `rustc -vV` commit-hash is not the pin's `rustc_commit`.
//
// The build aims to be reproducible: the toolchain is rust-toolchain.toml's channel
// (RUSTUP_TOOLCHAIN, as the source folder is outside this repository), the target dir sits outside
// the source tree, and the rustflags are --remap-path-prefix for the source, cargo's home and the
// target dir, plus on Windows `-C target-feature=+crt-static` (no VC++ runtime DLLs, as
// .github/scripts/pe-crt-imports.mjs checks) and `-C link-arg=/Brepro` (a hash where the link
// time would go).
// SOURCE_DATE_EPOCH is the upstream commit's time, read from the archive. A runner image with a
// newer linker (MSVC, Xcode or binutils) can still change the bytes, so the pinned sha256 is what
// a release trusts, not a rebuild.
//
// `tag` prints the pin's patched_tag and `toolchain` the toolchain channel, for the workflow.
//
// Node built-ins only, like its neighbours; tar, git and cargo run as child processes. Tests:
// `agent-browser-build.test.mjs` next to this file, run with
// `node --test 'crates/desktop/sidecars/*.test.mjs'`, by hand (the xtask check is frozen).

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { copyFile, mkdir, mkdtemp, readFile, readdir, realpath, rm, writeFile } from 'node:fs/promises';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { gunzipSync } from 'node:zlib';
import { ourAssetUrl, parsePin } from './agent-browser.mjs';
import { getBytes } from './fetch.mjs';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const PIN = join(REPO, 'crates', 'desktop', 'agent-browser.toml');
const PATCHES = join(REPO, 'crates', 'desktop', 'agent-browser', 'patches');
const TOOLCHAIN = join(REPO, 'rust-toolchain.toml');

/** The triples `build` takes: the six release targets, which the pin takes from our build. */
export const TARGETS = [
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu',
  'x86_64-pc-windows-msvc',
  'aarch64-pc-windows-msvc',
];

/** The binary's file name suffix on `triple`. */
const exe = (triple) => (triple.includes('-windows-') ? '.exe' : '');

const USAGE = [
  'usage: node crates/desktop/sidecars/agent-browser-build.mjs check',
  '       node crates/desktop/sidecars/agent-browser-build.mjs build --target <triple> --out <dir>',
  '       node crates/desktop/sidecars/agent-browser-build.mjs tag | toolchain',
].join('\n');

/** A command line that is not one of USAGE's; main exits 2 on it. */
class UsageError extends Error {}

/**
 * The command line, as `{ command }` or, for build, `{ command, target, out }`. Throws a
 * UsageError on anything else: an unknown command or flag, a flag twice or without its value,
 * or a target that is not one of TARGETS.
 *
 * @param {string[]} argv the arguments after the script's path
 */
export function parseArgs(argv) {
  const [command, ...rest] = argv;
  if (command === 'check' || command === 'tag' || command === 'toolchain') {
    if (rest.length > 0) throw new UsageError(`${command} takes no arguments\n${USAGE}`);
    return { command };
  }
  if (command !== 'build') throw new UsageError(USAGE);
  const options = {};
  for (let i = 0; i < rest.length; i += 2) {
    const [flag, value] = [rest[i], rest[i + 1]];
    if (flag !== '--target' && flag !== '--out') throw new UsageError(`unknown flag ${flag}\n${USAGE}`);
    if (value === undefined || value.startsWith('--')) throw new UsageError(`${flag} needs a value`);
    const key = flag.slice(2);
    if (Object.hasOwn(options, key)) throw new UsageError(`${flag} twice`);
    options[key] = value;
  }
  if (options.target === undefined || options.out === undefined) {
    throw new UsageError(`build needs --target and --out\n${USAGE}`);
  }
  if (!TARGETS.includes(options.target)) {
    throw new UsageError(`--target must be one of ${TARGETS.join(', ')}, not ${options.target}`);
  }
  return { command, target: options.target, out: options.out };
}

const PATCH = /^(\d{4})-[a-z0-9][a-z0-9-]*\.patch$/;

/**
 * The patch folder's file names in the order they apply: NNNN-<name>.patch, numbered 0001 up
 * with no gap. Dot files (.gitkeep) are skipped; any other file, a gap, a number twice or no
 * patch at all throws, so a misnamed patch is never left out silently.
 *
 * @param {string[]} names the folder's entries
 * @returns {string[]}
 */
export function orderPatches(names) {
  const patches = names.filter((name) => !name.startsWith('.'));
  for (const name of patches) {
    if (!PATCH.test(name)) throw new Error(`patches/${name} is not named NNNN-<name>.patch`);
  }
  if (patches.length === 0) throw new Error('patches/ holds no patch');
  const sorted = [...patches].sort();
  sorted.forEach((name, i) => {
    const want = String(i + 1).padStart(4, '0');
    if (PATCH.exec(name)[1] !== want) {
      throw new Error(`patches/${name} is out of sequence: patch ${i + 1} must start ${want}-`);
    }
  });
  return sorted;
}

/** The SHA-256 of `bytes`, lowercase hex. */
export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

/** Throws unless `bytes` hash to `expected`; `label` names them in the message. */
export function checkSha(bytes, expected, label) {
  const actual = sha256(bytes);
  if (actual !== expected) {
    throw new Error(`${label}: sha256 is ${actual}, the pin says ${expected}; refused`);
  }
}

/** A pax header's records, `<length> <key>=<value>\n` each, as an object. */
function paxRecords(data) {
  const records = {};
  let at = 0;
  while (at < data.length) {
    const space = data.indexOf(0x20, at);
    const length = Number(data.subarray(at, space).toString('ascii'));
    if (space < 0 || !Number.isInteger(length) || length <= 0) break;
    const record = data.subarray(space + 1, at + length - 1).toString('utf8');
    const equals = record.indexOf('=');
    records[record.slice(0, equals)] = record.slice(equals + 1);
    at += length;
  }
  return records;
}

/**
 * What the gzipped tar `gz` says about itself: `commit`, the commit id GitHub's archive puts in
 * its global pax header (null without one), `mtime`, the first entry's time in Unix seconds
 * (GitHub stamps every entry with the commit's time), and `symlinks`, the symbolic links'
 * names, which the extraction leaves out (Windows tar cannot always make them).
 *
 * @param {Buffer} gz
 * @returns {{ commit: string | null, mtime: number | null, symlinks: string[] }}
 */
export function tarInfo(gz) {
  const tar = gunzipSync(gz);
  let commit = null;
  let mtime = null;
  let pax = null; // the next entry's extended header, if any
  const symlinks = [];
  for (let at = 0; at + 512 <= tar.length; ) {
    const header = tar.subarray(at, at + 512);
    if (header.every((byte) => byte === 0)) break;
    const field = (from, to) => {
      const bytes = header.subarray(from, to);
      const end = bytes.indexOf(0);
      return bytes.subarray(0, end < 0 ? bytes.length : end).toString('utf8');
    };
    if (field(257, 262) !== 'ustar') throw new Error(`the archive has a non-ustar header at ${at}`);
    const size = parseInt(field(124, 136).trim() || '0', 8);
    const type = field(156, 157) || '0';
    const data = tar.subarray(at + 512, at + 512 + size);
    if (type === 'g') {
      commit = paxRecords(data).comment ?? commit;
    } else if (type === 'x') {
      pax = paxRecords(data);
    } else {
      const prefix = field(345, 500);
      const name = pax?.path ?? (prefix ? `${prefix}/${field(0, 100)}` : field(0, 100));
      mtime ??= parseInt(field(136, 148).trim(), 8);
      if (type === '2') symlinks.push(name);
      pax = null;
    }
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return { commit, mtime, symlinks };
}

/** rust-toolchain.toml's channel. */
export function toolchainChannel(text) {
  const match = /^channel\s*=\s*"([^"]+)"\s*$/m.exec(text);
  if (!match) throw new Error('rust-toolchain.toml has no channel = "..."');
  return match[1];
}

/**
 * The environment of the cargo build for `triple` (see the header): `env` without RUSTFLAGS,
 * plus the target dir, the encoded rustflags (0x1f between flags keeps a path with spaces whole),
 * SOURCE_DATE_EPOCH and RUSTUP_TOOLCHAIN.
 *
 * @param {{ triple: string, src: string, targetDir: string, epoch: number, channel: string,
 *   env?: Record<string, string | undefined>, home?: string }} where
 */
export function buildEnv({ triple, src, targetDir, epoch, channel, env = process.env, home = homedir() }) {
  const cargoHome = env.CARGO_HOME ?? join(home, '.cargo');
  const windows = triple.endsWith('-pc-windows-msvc');
  const flags = [
    ...(windows ? ['-C', 'target-feature=+crt-static', '-C', 'link-arg=/Brepro'] : []),
    `--remap-path-prefix=${src}=/agent-browser`,
    `--remap-path-prefix=${cargoHome}=/cargo`,
    `--remap-path-prefix=${targetDir}=/target`,
  ];
  const out = {
    ...env,
    CARGO_TARGET_DIR: targetDir,
    CARGO_ENCODED_RUSTFLAGS: flags.join('\x1f'),
    SOURCE_DATE_EPOCH: String(epoch),
    RUSTUP_TOOLCHAIN: channel,
  };
  delete out.RUSTFLAGS;
  return out;
}

/** The pin's two lines for `triple`, to paste under its table. */
export function pinLines(triple, pin, digest) {
  return `url = "${ourAssetUrl(triple, pin)}"\nsha256 = "${digest}"`;
}

/** Removes the temporary folder; a failure is logged, never thrown over the real error. */
async function cleanup(work, log) {
  await rm(work, { recursive: true, force: true, maxRetries: 3 }).catch((error) =>
    log(`agent-browser: could not remove ${work}: ${error.message}`),
  );
}

/** Runs `command`, its output on ours; throws unless it exits 0. */
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options });
  if (result.error) throw new Error(`${command} did not start: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} exited ${result.status}`);
}

/** The first line of `command`'s output, or null when it cannot run. */
function firstLine(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', ...options });
  if (result.error) return null;
  const text = `${result.stdout ?? ''}${result.stderr ?? ''}`;
  return text.split(/\r?\n/).find((line) => line.trim() !== '') ?? null;
}

/** The last line of `command`'s output (cc -v ends with its version), or null. */
function lastLine(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  if (result.error) return null;
  const lines = `${result.stdout ?? ''}${result.stderr ?? ''}`.split(/\r?\n/).filter((l) => l.trim());
  return lines.at(-1) ?? null;
}

/**
 * Downloads, checks and extracts the pinned source, then applies the patches, each after a
 * `git apply --check`. Returns the temporary folder (the caller removes it), the source folder
 * in it, the commit time and the patches with their hashes.
 */
async function prepare({ pin, patchDir, log, fetch }) {
  const names = orderPatches(await readdir(patchDir));
  const commit = pin.upstreamCommit;
  const url = `https://github.com/vercel-labs/agent-browser/archive/${commit}.tar.gz`;
  log(`downloading ${url}`);
  const gz = await getBytes(url, { label: 'agent-browser: the upstream source', fetch, log });
  checkSha(gz, pin.upstreamTarballSha256, 'agent-browser: the upstream source');
  const info = tarInfo(gz);
  if (info.commit !== commit) {
    throw new Error(`agent-browser: the archive names commit ${info.commit}, the pin ${commit}`);
  }

  const work = await realpath(await mkdtemp(join(tmpdir(), 'agent-browser-src-')));
  try {
    // tar runs in the folder with a relative name: GNU tar reads `C:` in a path as a host.
    await writeFile(join(work, 'upstream.tar.gz'), gz);
    const excludes = info.symlinks.map((name) => `--exclude=${name}`);
    run('tar', ['-xzf', 'upstream.tar.gz', ...excludes], { cwd: work });
    await rm(join(work, 'upstream.tar.gz'));
    const src = join(work, `agent-browser-${commit}`);
    if (!existsSync(join(src, 'cli', 'Cargo.toml'))) throw new Error(`${src} has no cli/Cargo.toml`);

    // The ceiling keeps git from finding a repository above the temporary folder.
    const git = { cwd: src, env: { ...process.env, GIT_CEILING_DIRECTORIES: work } };
    const patches = [];
    for (const name of names) {
      const path = join(patchDir, name);
      run('git', ['-c', 'core.autocrlf=false', 'apply', '--check', path], git);
      run('git', ['-c', 'core.autocrlf=false', 'apply', path], git);
      patches.push({ name, sha256: sha256(await readFile(path)) });
      log(`applied ${name}`);
    }
    return { work, src, epoch: info.mtime, patches };
  } catch (error) {
    await cleanup(work, log);
    throw error;
  }
}

/**
 * The linker's version: on Windows the MSVC tools version and link.exe's banner, as far as
 * vswhere finds them; on macOS `ld -v`; on Linux `cc -v` and `ld --version`. Nulls if not found.
 */
function linkerVersions(env) {
  if (process.platform === 'darwin') return { msvc_tools: null, link: firstLine('ld', ['-v']) };
  if (process.platform !== 'win32') {
    return { msvc_tools: null, link: firstLine('ld', ['--version']), cc: lastLine('cc', ['-v']) };
  }
  const none = { msvc_tools: null, link: null };
  const vswhere = join(env['ProgramFiles(x86)'] ?? '', 'Microsoft Visual Studio', 'Installer', 'vswhere.exe');
  if (!existsSync(vswhere)) return none;
  const install = firstLine(vswhere, ['-latest', '-products', '*', '-property', 'installationPath']);
  if (!install) return none;
  const versionFile = join(install, 'VC', 'Auxiliary', 'Build', 'Microsoft.VCToolsVersion.default.txt');
  const tools = existsSync(versionFile) ? readFileSync(versionFile, 'utf8').trim() : null;
  if (!tools) return none;
  for (const host of ['HostX64', 'HostARM64']) {
    for (const arch of ['x64', 'arm64']) {
      const link = join(install, 'VC', 'Tools', 'MSVC', tools, 'bin', host, arch, 'link.exe');
      if (existsSync(link)) return { msvc_tools: tools, link: firstLine(link, []) };
    }
  }
  return { msvc_tools: tools, link: null };
}

/** `build`: see the header. Returns the record it wrote. */
/** Throws unless `status`, `git status --porcelain` for the patches folder, is empty. */
export function checkCommitted(status, dir) {
  if (status.trim() !== '') {
    throw new Error(`agent-browser: ${dir} has uncommitted changes; build only from committed patches:\n${status.trim()}`);
  }
}

/** The 40-hex commit-hash in `rustc -vV`'s output, or null. */
export function rustcCommit(verbose) {
  return /^commit-hash: ([0-9a-f]{40})\s*$/m.exec(verbose)?.[1] ?? null;
}

async function build({ target, out }, { pin, patchDir, log, fetch }) {
  const channel = toolchainChannel(await readFile(TOOLCHAIN, 'utf8'));
  // Only committed patches: a build from a working tree with edits there is refused.
  const status = spawnSync('git', ['status', '--porcelain', '--', patchDir], { cwd: REPO, encoding: 'utf8' });
  if (status.error || status.status !== 0) {
    throw new Error(`agent-browser: git status failed for ${patchDir}; build only from a git checkout`);
  }
  checkCommitted(status.stdout, patchDir);
  const { work, src, epoch, patches } = await prepare({ pin, patchDir, log, fetch });
  try {
    const targetDir = join(work, 'target');
    const env = buildEnv({ triple: target, src, targetDir, epoch, channel });
    // The toolchain rustup gives for the channel must be the pinned one (rustc_commit).
    const verbose = spawnSync('rustc', ['-vV'], { encoding: 'utf8', env });
    const found = verbose.status === 0 ? rustcCommit(verbose.stdout) : null;
    if (found !== pin.rustcCommit) {
      throw new Error(`agent-browser: rustc's commit-hash is ${found}, the pin's rustc_commit is ${pin.rustcCommit}`);
    }
    // The tests first: a failing one stops the build, so nothing untested is published.
    run('cargo', ['test', '--locked', '--target', target], { cwd: join(src, 'cli'), env });
    run('cargo', ['build', '--release', '--locked', '--target', target], { cwd: join(src, 'cli'), env });

    const outDir = resolve(out);
    await mkdir(outDir, { recursive: true });
    const name = `agent-browser-${target}${exe(target)}`;
    const binary = join(outDir, name);
    await copyFile(join(targetDir, target, 'release', `agent-browser${exe(target)}`), binary);
    const bytes = await readFile(binary);
    const digest = sha256(bytes);
    const record = {
      triple: target,
      sha256: digest,
      size: bytes.length,
      patched_tag: pin.patchedTag,
      version: pin.version,
      upstream_commit: pin.upstreamCommit,
      upstream_tarball_sha256: pin.upstreamTarballSha256,
      patches,
      source_date_epoch: epoch,
      rustflags: env.CARGO_ENCODED_RUSTFLAGS.split('\x1f'),
      rustc: verbose.stdout.trim(),
      ...linkerVersions(process.env),
      runner_image: process.env.ImageOS ? `${process.env.ImageOS} ${process.env.ImageVersion}` : null,
    };
    await writeFile(join(outDir, `agent-browser-${target}.json`), `${JSON.stringify(record, null, 2)}\n`);
    log(`${name} sha256 ${digest} (${bytes.length} bytes)`);
    log(`the pin's lines for [${target}], once the release is published:`);
    log(pinLines(target, pin, digest));
    return record;
  } finally {
    await cleanup(work, log);
  }
}

/**
 * Runs one command line. `options` replace the pin, the patch folder, the log and the network,
 * for the tests.
 */
export async function main(argv, options = {}) {
  const { pinPath = PIN, patchDir = PATCHES, log = console.log, fetch } = options;
  const args = parseArgs(argv);
  if (args.command === 'toolchain') {
    log(toolchainChannel(await readFile(TOOLCHAIN, 'utf8')));
    return;
  }
  const pin = parsePin(await readFile(pinPath, 'utf8'));
  if (args.command === 'tag') {
    log(pin.patchedTag);
    return;
  }
  if (args.command === 'check') {
    const { work, patches } = await prepare({ pin, patchDir, log, fetch });
    await cleanup(work, log);
    log(`ok: ${patches.length} patch(es) apply cleanly to ${pin.upstreamCommit} (v${pin.version})`);
    return;
  }
  await build(args, { pin, patchDir, log, fetch });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exit(error instanceof UsageError ? 2 : 1);
  });
}
