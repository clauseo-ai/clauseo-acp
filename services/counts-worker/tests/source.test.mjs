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

/** Code that could reach a header, the request's metadata or the raw JS request another way. */
const BANNED = [
  [/\bcf\b/i, "the cf object or the Cf type"],
  [/\.keys\s*\(/, ".keys()"],
  [/\.values\s*\(/, ".values()"],
  [/\.entries\s*\(/, ".entries()"],
  [/\bget_all\b/, "get_all"],
  [/\bheaders_mut\b/, "headers_mut"],
  [/\.inner\s*\(/, ".inner()"],
  [/\.url\s*\(/, ".url()"],
  [/\.query\b/, ".query"],
  [/\bweb_sys\b/, "web_sys"],
  [/\bworker_sys\b/, "worker_sys"],
  [/\bjs_sys\b/, "js_sys"],
  [/\bReflect\b/, "Reflect"],
  [/\b(JsCast|dyn_into|dyn_ref|unchecked_into|unchecked_ref)\b/, "a JS cast"],
  [/\bhttp::/, "the http crate's Request"],
  [/\bworker::\*/, "a glob import of worker"],
];

/**
 * Every way the Rust in `sources` ({ path: text }) could read a request header or the request's
 * metadata, outside comments, other than the one allowed: `glue.rs`'s `fn header`, which calls
 * `.headers().get(which.name())`, and `RequestHeader`, whose names are the two allowed.
 */
export function headerReads(sources) {
  const found = [];
  // Comments read nothing, and the docs name headers freely.
  const all = Object.entries(sources).map(([path, text]) => [path, text.replace(/\/\/[^\n]*/g, "")]);
  const uses = all.flatMap(([path, text]) => [...text.matchAll(/\bheaders\b/g)].map((m) => ({ path, at: m.index, text })));
  if (uses.length !== 1) found.push(`\`headers\` appears ${uses.length} times; only fn header may name it`);
  for (const { path, at, text } of uses) {
    const before = text.slice(0, at);
    const fn = before.lastIndexOf("fn ");
    const inHeader = path.endsWith("glue.rs") && before.slice(fn).startsWith("fn header(");
    if (!inHeader || !text.slice(at - 1).startsWith(".headers().get(which.name())")) {
      found.push(`${path}: \`headers\` outside fn header, or not as .headers().get(which.name())`);
    }
  }
  let impls = 0;
  for (const [path, text] of all) {
    for (const impl of text.matchAll(/impl RequestHeader \{([\s\S]*?)\n\}/g)) {
      impls += 1;
      const names = [...impl[1].matchAll(/"([^"]*)"/g)].map((m) => m[1]).sort();
      if (JSON.stringify(names) !== JSON.stringify(["Content-Encoding", "Content-Length"])) {
        found.push(`${path}: RequestHeader names ${JSON.stringify(names)}`);
      }
    }
    for (const [pattern, what] of BANNED) {
      if (pattern.test(text)) found.push(`${path}: ${what}`);
    }
  }
  if (impls !== 1) found.push(`${impls} impl RequestHeader blocks; there must be one`);
  return found;
}

test("the source reads only Content-Encoding and Content-Length", () => {
  const sources = Object.fromEntries(rust.map((path) => [relative(root, path), readFileSync(path, "utf8")]));
  assert.deepEqual(headerReads(sources), []);
});

test("the header check finds each planted way round it", () => {
  const good = readFileSync(join(root, "src", "glue.rs"), "utf8");
  const append = (code) => good + "\n" + code + "\n";
  const planted = {
    "a third name": good.replace('"Content-Length"', '"User-Agent"'),
    "a second read": append('fn agent(r: &Request) { r.headers().get("User-Agent"); }'),
    "headers as a plain call": append('fn agent(r: &Request) { Request::headers(r).get("User-Agent"); }'),
    "cf as a plain call": append("fn place(r: &Request) { Request::cf(r); }"),
    "cf as a method": append("fn place(r: &Request) { r.cf(); }"),
    "the Cf type": append("fn place(c: &worker::Cf) {}"),
    "an alias of web_sys": append("use worker::worker_sys::web_sys as w;"),
    "web_sys by path": append("fn raw(r: &Request) -> &web_sys::Request { todo() }"),
    "headers.keys()": good.replace(".headers().get(which.name())", ".headers().get(which.name()); h.keys()"),
    "headers.values()": append("fn all(h: &worker::Headers) { h.values(); }"),
    "headers.entries()": append("fn all(h: &worker::Headers) { h.entries(); }"),
    "the raw request": append("fn all(r: &Request) { r.inner(); }"),
    "the url": append("fn url(r: &Request) { r.url(); }"),
    "a JS cast": append("use worker::wasm_bindgen::JsCast;"),
    "a glob import": append("use worker::*;"),
    "headers in another file": null,
    "no RequestHeader": good.replace("impl RequestHeader {", "impl Header {"),
  };
  for (const [name, text] of Object.entries(planted)) {
    const sources = text === null
      ? { "src/glue.rs": good, "src/other.rs": "fn x(r: &Request) { r.headers(); }" }
      : { "src/glue.rs": text };
    assert.notDeepEqual(headerReads(sources), [], name);
  }
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
