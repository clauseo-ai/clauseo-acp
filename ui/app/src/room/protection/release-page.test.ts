// The download address lives in one constant, RELEASES_PAGE (installer.ts). Every link in the
// install guides that tells a person where to download Clauseo points at the latest release of
// that page, and no guide points at the old download site. When RELEASES_PAGE changes, this test
// names every guide line that still needs the same change.
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { RELEASES_PAGE, installerPage } from "./installer.ts";

// This file sits in ui/app/src/room/protection/, five levels below the repo root.
const REPO = join(dirname(fileURLToPath(import.meta.url)), "../../../../..");
const INSTALL_DIR = join(REPO, "docs/install");

const RELEASES_LINK = /(https?:\/\/)?github\.com\/clauseo-ai\/clauseo-acp\/releases(\/[\w.-]+)*/g;
const OLD_DOWNLOAD = /clauseo\.chat\/download/g;

const mdFiles = readdirSync(INSTALL_DIR, { recursive: true })
  .map(String)
  .filter((f) => f.endsWith(".md"))
  .sort();

/** Every line of every install guide, as `file:line` with its text. */
const lines = mdFiles.flatMap((file) =>
  readFileSync(join(INSTALL_DIR, file), "utf8")
    .split("\n")
    .map((text, i) => ({ where: `docs/install/${file}:${i + 1}`, text })),
);

const found = (re: RegExp) =>
  lines.flatMap(({ where, text }) => [...text.matchAll(re)].map((m) => ({ where, found: m[0] })));

test("the install guides are found where the test looks for them", () => {
  expect(mdFiles).toEqual(expect.arrayContaining(["macos.md", "windows.md"]));
});

test("every releases link in the install guides is the latest release page", () => {
  const wrong = found(RELEASES_LINK)
    .filter((h) => h.found !== `${RELEASES_PAGE}/latest`)
    .map((h) => `${h.where}: found "${h.found}", expected "${RELEASES_PAGE}/latest"`);
  expect(wrong.join("\n")).toBe("");
});

test("no install guide links the old download site", () => {
  const old = found(OLD_DOWNLOAD).map((h) => `${h.where}: found "${h.found}"`);
  expect(old.join("\n")).toBe("");
});

test.each(["macos.md", "windows.md"])("%s links the latest release page", (file) => {
  const links = found(RELEASES_LINK).filter(
    (h) => h.where.startsWith(`docs/install/${file}:`) && h.found === `${RELEASES_PAGE}/latest`,
  );
  expect(links.length).toBeGreaterThanOrEqual(1);
});

test("the installer's page starts with RELEASES_PAGE", () => {
  expect(installerPage("1.2.3")?.startsWith(RELEASES_PAGE)).toBe(true);
});
