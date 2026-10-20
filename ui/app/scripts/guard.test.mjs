import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  ConflictError,
  FileGuard,
  guardPaths,
  leftover,
  recoverFile,
  restoreFailure,
  tmpOf,
} from "./guard.mjs";

const REL = "src/notes.txt";
const ORIGINAL = "the original\n";
/** The demo tool's own edit: one line appended. */
const EDIT = "// demo edit\n";
const isOwnEdit = (current, copy) => current === `${copy}${EDIT}`;

const dirs = [];

afterEach(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** The guard paths of REL in a fresh repo under the OS temp dir, removed after each test. */
function paths() {
  const root = mkdtempSync(join(tmpdir(), "guard-"));
  dirs.push(root);
  return guardPaths({
    root,
    rel: REL,
    backupDir: "target/xtask/demo",
    tool: "demo",
    rerun: "cargo xtask demo",
  });
}

function put(path, text) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
}

/** A repo whose REL holds `text`. */
function repo(text = ORIGINAL) {
  const at = paths();
  put(at.file, text);
  return at;
}

/** A repo whose REL holds `text`, with the copy an interrupted run left. */
function interrupted(text) {
  const at = repo(text);
  put(at.backup, ORIGINAL);
  return at;
}

const read = (path) => readFileSync(path, "utf8");

describe("guardPaths and tmpOf", () => {
  it("put the copy under the backup folder and name the tool in temporary files", () => {
    const at = guardPaths({ root: "/r", rel: REL, backupDir: "b/c", tool: "t", rerun: "x" });
    expect(at).toEqual({
      rel: REL,
      file: join("/r", REL),
      base: join("/r", "b/c"),
      backup: join("/r", "b/c", REL),
      backupRel: `b/c/${REL}`,
      tool: "t",
      rerun: "x",
    });
    expect(tmpOf(at.file, at.tool)).toBe(join("/r", "src/.notes.txt.t.tmp"));
  });
});

describe("leftover", () => {
  const copy = Buffer.from(ORIGINAL);
  it("tells clean, the tool's own edit, missing and any other change apart", () => {
    expect(leftover(Buffer.from(ORIGINAL), copy, isOwnEdit)).toBe("clean");
    expect(leftover(Buffer.from(`${ORIGINAL}${EDIT}`), copy, isOwnEdit)).toBe("restore");
    expect(leftover(undefined, copy, isOwnEdit)).toBe("missing");
    expect(leftover(Buffer.from("other\n"), copy, isOwnEdit)).toBe("conflict");
  });
});

describe("recoverFile", () => {
  it("puts the copy back over the tool's own edit, and removes the emptied folders", () => {
    const at = interrupted(`${ORIGINAL}${EDIT}`);
    expect(recoverFile(at, isOwnEdit)).toBe(`restored ${REL} from an interrupted demo run`);
    expect(read(at.file)).toBe(ORIGINAL);
    expect(existsSync(at.backup)).toBe(false);
    expect(existsSync(join(at.base, "src"))).toBe(false);
    expect(existsSync(at.base)).toBe(true);
  });

  it("does nothing without a copy, and drops a copy of an unchanged file", () => {
    const at = repo();
    expect(recoverFile(at, isOwnEdit)).toBeUndefined();
    put(at.backup, ORIGINAL);
    expect(recoverFile(at, isOwnEdit)).toBe(
      `deleted the copy of ${REL} an interrupted demo run left; the file was unchanged`,
    );
    expect(read(at.file)).toBe(ORIGINAL);
    expect(existsSync(at.backup)).toBe(false);
  });

  it("deletes the temporary files of an interrupted write", () => {
    const at = repo();
    put(tmpOf(at.file, at.tool), "half");
    put(tmpOf(at.backup, at.tool), "half");
    expect(recoverFile(at, isOwnEdit)).toBeUndefined();
    expect(existsSync(tmpOf(at.file, at.tool))).toBe(false);
    expect(existsSync(tmpOf(at.backup, at.tool))).toBe(false);
  });

  it("keeps a half-written file and its copy, and reports the conflict", () => {
    const at = interrupted(ORIGINAL.slice(0, 5));
    let error;
    try {
      recoverFile(at, isOwnEdit);
    } catch (caught) {
      error = caught;
    }
    expect(error).toBeInstanceOf(ConflictError);
    expect(error.message).toBe(
      `${REL} changed after an interrupted demo run; its saved copy is target/xtask/demo/${REL}`,
    );
    expect(error.fix).toBe(
      `compare the two, keep what you want, then delete target/xtask/demo/${REL}`,
    );
    expect(read(at.file)).toBe(ORIGINAL.slice(0, 5));
    expect(read(at.backup)).toBe(ORIGINAL);
  });

  it("calls a missing file a conflict and keeps the copy", () => {
    const at = interrupted(ORIGINAL);
    rmSync(at.file);
    expect(() => recoverFile(at, isOwnEdit)).toThrow(
      `${REL} is missing after an interrupted demo run; its saved copy is target/xtask/demo/${REL}`,
    );
    expect(read(at.backup)).toBe(ORIGINAL);
  });
});

describe("FileGuard", () => {
  it("puts the original back after its edits and leaves no copy or temporary file", () => {
    const at = repo();
    const guard = new FileGuard(at);
    expect(read(at.backup)).toBe(ORIGINAL);
    guard.write("one\n");
    guard.write("two\n");
    expect(read(at.file)).toBe("two\n");
    expect(existsSync(tmpOf(at.file, at.tool))).toBe(false);
    guard.restore();
    guard.restore();
    expect(read(at.file)).toBe(ORIGINAL);
    expect(existsSync(at.backup)).toBe(false);
  });

  it("keeps a developer's edit made during the run, and both files", () => {
    const at = repo();
    const guard = new FileGuard(at);
    guard.write("one\n");
    put(at.file, "typed during the run\n");
    expect(() => guard.write("two\n")).toThrow(
      `${REL} changed during this demo run; its saved copy is target/xtask/demo/${REL}`,
    );
    expect(() => guard.write("three\n")).toThrow(`${REL} is no longer guarded`);
    guard.restore();
    expect(read(at.file)).toBe("typed during the run\n");
    expect(read(at.backup)).toBe(ORIGINAL);
  });

  it("keeps an edit made just before the restore, and a file that went missing", () => {
    const edited = repo();
    const guard = new FileGuard(edited);
    guard.write("one\n");
    put(edited.file, "export {};\n");
    expect(() => guard.restore()).toThrow(ConflictError);
    expect(read(edited.file)).toBe("export {};\n");
    expect(read(edited.backup)).toBe(ORIGINAL);

    const gone = repo();
    const second = new FileGuard(gone);
    rmSync(gone.file);
    expect(() => second.restore()).toThrow(
      `${REL} went missing during this demo run; its saved copy is target/xtask/demo/${REL}`,
    );
    expect(read(gone.backup)).toBe(ORIGINAL);
  });

  it("refuses to start over an existing copy and leaves the file alone", () => {
    const at = repo();
    put(at.backup, "an older copy");
    expect(() => new FileGuard(at)).toThrow(
      `a saved copy of ${REL} already exists at target/xtask/demo/${REL}`,
    );
    expect(read(at.backup)).toBe("an older copy");
    expect(read(at.file)).toBe(ORIGINAL);
  });
});

describe("restoreFailure", () => {
  it("passes a conflict on and names the copy and the rerun for any other error", () => {
    const at = guardPaths({ root: "/r", rel: REL, backupDir: "b", tool: "t", rerun: "cmd x" });
    const conflict = new ConflictError("changed", "compare them");
    expect(restoreFailure(at, conflict)).toEqual({ message: "changed", fix: "compare them" });
    const eperm = new Error("EPERM: operation not permitted, rename");
    expect(restoreFailure(at, eperm)).toEqual({
      message: `could not put back ${REL}: EPERM: operation not permitted, rename; its saved copy is b/${REL}`,
      fix: `the next \`cmd x\` restores ${REL} from b/${REL}; or copy it back by hand`,
    });
  });
});
