// Ties the exact-name overrides of ui/app/.oxlintrc.json to its `scripts/**` rules. An override
// that sets no-restricted-imports replaces the whole rule for its files, so each restates the
// `scripts/**` bans; this test fails when one of them drops anything but its own door, so a ban
// added to `scripts/**` and missed in an override fails here, not in review.

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const RULE = "no-restricted-imports";

/** The modules each door lets its files import. */
const DOOR_MODULES = {
  write: ["node:fs", "fs", "node:fs/promises", "fs/promises"],
  process: ["node:child_process", "child_process"],
};

/** Each exempt file, and the doors it may use. Every other ban of `scripts/**` applies to it. */
const DOORS = {
  "scripts/guard.mjs": ["write"],
  "scripts/buildtime-proc.mjs": ["process"],
  "scripts/buildtime-leader.mjs": ["process"],
  "scripts/buildtime-lib.test.mjs": ["write"],
  "scripts/guard.test.mjs": ["write"],
  "scripts/oxlint-bans.test.mjs": ["write"],
  "scripts/buildtime-proc.test.mjs": ["write", "process"],
};

/** The import rule `scripts/**` sets with the paths of `doors` taken out. */
function withoutDoors(scripts, doors) {
  const open = new Set(doors.flatMap((door) => DOOR_MODULES[door]));
  const [level, options] = scripts;
  return [level, { ...options, paths: options.paths.filter((path) => !open.has(path.name)) }];
}

/**
 * What is wrong with `config`'s overrides, one line each: an unknown override, an exempt file
 * this test does not know or that does not exist, an override that sets more than the import
 * rule, or an import rule that differs from `scripts/**` minus the file's doors.
 */
function overrideProblems(config) {
  const problems = [];
  const [transport, scripts, ...exact] = config.overrides;
  if (transport?.files?.join() !== "src/transport/**" || scripts?.files?.join() !== "scripts/**") {
    return ["the first two overrides must be src/transport/** and then scripts/**"];
  }
  const seen = new Set();
  for (const override of exact) {
    const rules = Object.keys(override.rules ?? {});
    if (rules.join() !== RULE) {
      problems.push(`${override.files.join(", ")}: sets ${rules.join(", ")}, not only ${RULE}`);
    }
    for (const file of override.files) {
      seen.add(file);
      if (DOORS[file] === undefined) {
        problems.push(`${file}: not an exempt file in DOORS of oxlint-overrides.test.mjs`);
        continue;
      }
      if (!existsSync(join(APP_DIR, file))) {
        problems.push(`${file}: exempt in .oxlintrc.json but missing`);
      }
      const want = withoutDoors(scripts.rules[RULE], DOORS[file]);
      if (JSON.stringify(override.rules[RULE]) !== JSON.stringify(want)) {
        const have = new Set(
          (override.rules[RULE]?.[1]?.paths ?? []).map((path) => JSON.stringify(path)),
        );
        const differ = want[1].paths.filter((path) => !have.has(JSON.stringify(path)));
        const names = differ.map((path) => path.name).join(", ");
        problems.push(
          `${file}: its ${RULE} is not the scripts/** one without the ${DOORS[file].join(" and ")} door${differ.length > 0 ? `; it lacks or changes ${names}` : ""}`,
        );
      }
    }
  }
  for (const file of Object.keys(DOORS).filter((name) => !seen.has(name))) {
    problems.push(`${file}: in DOORS but has no override`);
  }
  return problems;
}

const config = () => JSON.parse(readFileSync(join(APP_DIR, ".oxlintrc.json"), "utf8"));

describe("the exact-name overrides of .oxlintrc.json", () => {
  it("each restate the scripts/** import bans without their own door", () => {
    expect(overrideProblems(config())).toEqual([]);
  });

  it("fail when scripts/** gains a module an override lacks", () => {
    const changed = config();
    changed.overrides[1].rules[RULE][1].paths.push({ name: "node:dgram", message: "planted" });
    const lacks = (file, door) =>
      `${file}: its ${RULE} is not the scripts/** one without the ${door} door; it lacks or changes node:dgram`;
    expect(overrideProblems(changed)).toEqual([
      lacks("scripts/guard.mjs", "write"),
      lacks("scripts/buildtime-proc.mjs", "process"),
      lacks("scripts/buildtime-leader.mjs", "process"),
      lacks("scripts/buildtime-lib.test.mjs", "write"),
      lacks("scripts/guard.test.mjs", "write"),
      lacks("scripts/oxlint-bans.test.mjs", "write"),
      lacks("scripts/buildtime-proc.test.mjs", "write and process"),
    ]);
  });

  it("fail when scripts/** bans a node:fs name the process door files lack", () => {
    const changed = config();
    const fs = changed.overrides[1].rules[RULE][1].paths.find((path) => path.name === "node:fs");
    fs.importNames.push("planted");
    const lacks = (file) =>
      `${file}: its ${RULE} is not the scripts/** one without the process door; it lacks or changes node:fs`;
    expect(overrideProblems(changed)).toEqual([
      lacks("scripts/buildtime-proc.mjs"),
      lacks("scripts/buildtime-leader.mjs"),
    ]);
  });

  it("fail on an override that turns another rule off, or exempts an unknown file", () => {
    const changed = config();
    changed.overrides[2].rules["no-eval"] = "off";
    changed.overrides.push({ files: ["scripts/new.mjs"], rules: { [RULE]: "off" } });
    expect(overrideProblems(changed)).toEqual([
      `scripts/guard.mjs: sets ${RULE}, no-eval, not only ${RULE}`,
      "scripts/new.mjs: not an exempt file in DOORS of oxlint-overrides.test.mjs",
    ]);
  });
});
