import {
  chmodSync,
  existsSync,
  linkSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  ConflictError,
  ensureLink,
  FileGuard,
  guardPaths,
  isRepoPath,
  leftover,
  recoverFile,
  restoreFailure,
  tmpOf,
  writeAll,
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

describe("guardPaths refusals", () => {
  const at = (rel, backupDir = "target/x", tool = "t") =>
    guardPaths({ root: "/r", rel, backupDir, tool, rerun: "x" });

  it("refuses a file or backup folder outside the repo, as xtask's is_repo_path does", () => {
    expect(isRepoPath("ui/app/src/App.tsx")).toBe(true);
    for (const rel of [
      "../x",
      "a/../../x",
      "/etc/x",
      "C:\\x",
      "c:x",
      "a\\..\\..\\x",
      "./a",
      "a//b",
      "",
    ]) {
      expect(() => at(rel)).toThrow(
        `refusing to guard \`${rel}\`: it is not a relative path inside the repo; pass a repo path, such as ui/app/src/App.tsx`,
      );
    }
    expect(() => at("a.txt", "../target")).toThrow("refusing to guard `../target`");
  });

  it("refuses a tool name that would put the temporary file elsewhere", () => {
    expect(() => at("a.txt", "target/x", "../t")).toThrow(
      "refusing the tool name `../t`: it names temporary files, so use lowercase letters, digits and -",
    );
  });
});

describe("writeAll", () => {
  it("writes the rest after a short write, and throws on a write that makes no progress", () => {
    const bytes = Buffer.from("0123456789");
    const got = [];
    writeAll(7, bytes, (fd, buffer, offset, length) => {
      expect(fd).toBe(7);
      const n = Math.min(3, length);
      got.push(buffer.subarray(offset, offset + n).toString());
      return n;
    });
    expect(got).toEqual(["012", "345", "678", "9"]);
    expect(() => writeAll(7, bytes, () => 0)).toThrow("a write of 10 bytes made no progress");
  });
});

describe("FileGuard's copy and modes", () => {
  it("never replaces a copy another run made between the check and the create", () => {
    const at = repo();
    const theirs = "the other run's original\n";
    const raceLink = (from, to) => {
      put(to, theirs);
      linkSync(from, to);
    };
    let error;
    try {
      new FileGuard(at, { link: raceLink });
    } catch (caught) {
      error = caught;
    }
    expect(error).toBeInstanceOf(ConflictError);
    expect(error.message).toBe(`a saved copy of ${REL} already exists at target/xtask/demo/${REL}`);
    expect(read(at.backup)).toBe(theirs);
    expect(read(at.file)).toBe(ORIGINAL);
    expect(existsSync(tmpOf(at.backup, at.tool))).toBe(false);
  });

  it("keeps a temporary copy left by an interrupted save, and edits nothing", () => {
    const at = repo();
    put(tmpOf(at.backup, at.tool), "half");
    expect(() => new FileGuard(at)).toThrow(
      `could not back up ${REL}: target/xtask/demo/src/.notes.txt.demo.tmp is left from an interrupted save`,
    );
    expect(read(tmpOf(at.backup, at.tool))).toBe("half");
    expect(existsSync(at.backup)).toBe(false);
    expect(read(at.file)).toBe(ORIGINAL);
  });

  it.skipIf(process.platform === "win32")(
    "keeps the file's mode through a write and the restore",
    () => {
      const at = repo();
      chmodSync(at.file, 0o751);
      const guard = new FileGuard(at);
      guard.write("one\n");
      expect(statSync(at.file).mode & 0o7777).toBe(0o751);
      guard.restore();
      expect(statSync(at.file).mode & 0o7777).toBe(0o751);
      expect(read(at.file)).toBe(ORIGINAL);
    },
  );
});

describe("ensureLink", () => {
  /** A fresh repo under the OS temp dir with the folder ui/components, removed after each test. */
  function linkRepo() {
    const root = mkdtempSync(join(tmpdir(), "guard-link-"));
    dirs.push(root);
    mkdirSync(join(root, "ui", "components", "src"), { recursive: true });
    mkdirSync(join(root, "ui", "app"));
    return root;
  }
  const at = { rel: "ui/app/components", target: "../components" };

  it("makes the link where nothing is, then keeps it", () => {
    const root = linkRepo();
    expect(ensureLink({ root, ...at })).toBe("made");
    const link = join(root, "ui", "app", "components");
    expect(lstatSync(link).isSymbolicLink()).toBe(true);
    expect(realpathSync(link)).toBe(realpathSync(join(root, "ui", "components")));
    expect(ensureLink({ root, ...at })).toBe("kept");
  });

  it("never replaces a file, a folder or a link elsewhere at the path", () => {
    const root = linkRepo();
    const path = join(root, "ui", "app", "components");
    // git's stand-in for a link on a checkout without symlink support
    writeFileSync(path, "../components");
    expect(() => ensureLink({ root, ...at })).toThrow("something else is there");
    expect(read(path)).toBe("../components");
    rmSync(path);
    mkdirSync(path);
    expect(() => ensureLink({ root, ...at })).toThrow("something else is there");
    rmSync(path, { recursive: true });
    symlinkSync("../app", path, "junction");
    expect(() => ensureLink({ root, ...at })).toThrow("something else is there");
  });

  it("refuses a path outside the repo and a target that is no folder", () => {
    const root = linkRepo();
    expect(() => ensureLink({ root, rel: "../components", target: "app" })).toThrow(
      "not a relative path inside the repo",
    );
    expect(() => ensureLink({ root, rel: at.rel, target: "../missing" })).toThrow(
      "is not a folder",
    );
    expect(existsSync(join(root, "ui", "app", "components"))).toBe(false);
  });
});
