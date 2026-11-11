// node --test tests/source.test.mjs (no wrangler needed): the Worker's source reads no request
// header but Content-Encoding and Content-Length, and names no client address, no request
// metadata and no logging, in any case.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** Every file under `dir`, recursively. */
function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

const rust = files(join(root, "src")).filter((path) => path.endsWith(".rs"));
const scanned = [...rust, ...files(join(root, "migrations")), join(root, "wrangler.toml"), join(root, "Cargo.toml")];

/** Strings the Worker's files must never hold, matched in any case. */
export const FORBIDDEN = [
  "CF-Connecting-IP",
  "X-Forwarded-For",
  "True-Client-IP",
  "CF-Connecting-IPv6",
  "CF-Pseudo-IPv4",
  "X-Real-IP",
  ".cf(",
  "console_",
  "println!",
  "eprintln!",
  "dbg!",
  "tracing",
  "log::",
];

/** Each forbidden string in `text`, any case. */
function forbiddenIn(text) {
  const lower = text.toLowerCase();
  return FORBIDDEN.filter((word) => lower.includes(word.toLowerCase()));
}

/**
 * Every way the Rust in `sources` ({ path: text }) could read a request header, or the request's
 * metadata, other than the one `header` function with its two names.
 */
export function headerReads(sources) {
  const found = [];
  const all = Object.entries(sources);
  const calls = all.flatMap(([path, text]) => [...text.matchAll(/\.headers\s*\(\s*\)/g)].map((m) => ({ path, at: m.index, text })));
  if (calls.length !== 1) found.push(`.headers() is called ${calls.length} times; only fn header may call it`);
  for (const { path, at, text } of calls) {
    const before = text.slice(0, at);
    const fn = before.lastIndexOf("fn ");
    if (!before.slice(fn).startsWith("fn header(") || !text.slice(at).startsWith(".headers().get(which.name())")) {
      found.push(`${path}: .headers() outside fn header, or not as .headers().get(which.name())`);
    }
  }
  for (const [path, text] of all) {
    const impl = text.match(/impl RequestHeader \{([\s\S]*?)\n\}/);
    if (impl) {
      const names = [...impl[1].matchAll(/"([^"]*)"/g)].map((m) => m[1]).sort();
      if (JSON.stringify(names) !== JSON.stringify(["Content-Encoding", "Content-Length"])) {
        found.push(`${path}: RequestHeader names ${JSON.stringify(names)}`);
      }
    }
    for (const pattern of [
      /\.headers_mut\s*\(/,
      /\.get_all\s*\(/,
      /\.entries\s*\(\s*\)/,
      /\.inner\s*\(\s*\)/,
      /\.url\s*\(\s*\)/,
      /\.query\s*[(:]/,
      /web_sys::Request\b/,
      /\bReflect\b/,
    ]) {
      if (pattern.test(text)) found.push(`${path}: ${pattern}`);
    }
  }
  if (!all.some(([, text]) => text.includes("impl RequestHeader {"))) found.push("no impl RequestHeader");
  return found;
}

test("the source reads only Content-Encoding and Content-Length", () => {
  const sources = Object.fromEntries(rust.map((path) => [relative(root, path), readFileSync(path, "utf8")]));
  assert.deepEqual(headerReads(sources), []);
});

test("the header check finds a planted read", () => {
  const good = readFileSync(join(root, "src", "glue.rs"), "utf8");
  const planted = [
    good.replace('"Content-Length"', '"User-Agent"'),
    good + '\nfn agent(r: &Request) { r.headers().get("User-Agent"); }\n',
    good + "\nfn all(r: &Request) { r.inner(); }\n",
    good + "\nfn url(r: &Request) { r.url(); }\n",
    good.replace("impl RequestHeader {", "impl Header {"),
  ];
  for (const text of planted) assert.notDeepEqual(headerReads({ "src/glue.rs": text }), []);
});

test("no file names a client address, request metadata or logging, in any case", () => {
  const found = scanned.flatMap((path) => forbiddenIn(readFileSync(path, "utf8")).map((word) => `${relative(root, path)}: ${word}`));
  assert.deepEqual(found, []);
});

test("the grep finds each word in any case", () => {
  for (const word of FORBIDDEN) {
    assert.ok(forbiddenIn(`x ${word.toUpperCase()} y`).includes(word), word);
    assert.ok(forbiddenIn(`x ${word.toLowerCase()} y`).includes(word), word);
  }
  assert.deepEqual(forbiddenIn("x Content-Length y"), []);
  assert.deepEqual(forbiddenIn('req.headers().get("cf-connecting-ip")'), ["CF-Connecting-IP"]);
});
