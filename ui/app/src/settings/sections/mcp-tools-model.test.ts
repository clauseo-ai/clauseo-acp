// Settings › Tools' pure parts: the model's helpers (full paths, commands, file names, hosts), the list's detail for a
// launcher, and an import's lines with each left-out tool's reason. The section's states are mcp-tools.test.tsx's.
import { expect, test } from "vitest";
import { importLines } from "./mcp-tools-refusal.ts";
import {
  fileName,
  hostOf,
  isCommand,
  isFullPath,
  localDetail,
  type ImportResult,
} from "./tools-model.ts";

const RESULT: ImportResult = { command: "c-1", source: "claude_code", imported: [], skipped: [] };

test("the model's helpers: full paths, commands, file names, hosts", () => {
  expect(isFullPath("/opt/homebrew/bin/npx")).toBe(true);
  expect(isFullPath("C:\\Program Files\\nodejs\\npx.cmd")).toBe(true);
  // a network path is no path on this computer: the host refuses it as mcp.command_invalid
  expect(isFullPath("\\\\server\\tools\\run.exe")).toBe(false);
  expect(isCommand("\\\\server\\share\\run.exe")).toBe(false);
  expect(isFullPath("npx")).toBe(false);
  expect(isFullPath("./bin/npx")).toBe(false);
  expect(fileName("C:\\Program Files\\nodejs\\npx.cmd")).toBe("npx.cmd");
  expect(fileName("/usr/local/bin/uvx")).toBe("uvx");
  expect(hostOf("https://mcp.example.com:8443/x")).toBe("mcp.example.com:8443");
  expect(hostOf("not an address")).toBe("not an address");
  expect(isCommand("npx")).toBe(true);
  expect(isCommand("C:\\Program Files\\nodejs\\npx.cmd")).toBe(true);
  expect(isCommand("bin/server")).toBe(false);
  expect(isCommand("bin\\server.exe")).toBe(false);
  expect(isCommand("")).toBe(false);
});

test("the list's detail: a launcher's first argument that isn't a flag, else the program's file name", () => {
  expect(localDetail("/opt/homebrew/bin/npx", ["-y", "court-diary-mcp"])).toBe("court-diary-mcp");
  expect(localDetail("npx", ["-y", "@modelcontextprotocol/server-filesystem", "/Users/r"])).toBe(
    "server-filesystem",
  );
  expect(localDetail("C:\\Program Files\\nodejs\\npx.cmd", ["--yes", "docket-mcp"])).toBe(
    "docket-mcp",
  );
  expect(localDetail("uvx", ["notes-mcp", "--folder", "/Users/r"])).toBe("notes-mcp");
  expect(localDetail("/usr/bin/python3", ["-m", "notes_server"])).toBe("notes_server");
  expect(localDetail("node", ["/Users/r/mcp/pdf-tools/index.js"])).toBe("index.js");
  expect(localDetail("docker", ["run", "-i", "--rm", "-e", "TOKEN=x", "ghcr.io/acme/mcp"])).toBe(
    "mcp",
  );
  expect(localDetail("pnpm", ["dlx", "court-diary-mcp"])).toBe("court-diary-mcp");
  for (const launcher of ["bunx", "pnpx", "python", "deno"])
    expect(localDetail(launcher, ["--flag", "the-server"])).toBe("the-server");
  // a launcher with nothing named, and any other program: its own file name
  expect(localDetail("/usr/local/bin/uvx", [])).toBe("uvx");
  expect(localDetail("/Users/r/tools/filings-server", ["--port", "9"])).toBe("filings-server");
});

test("the import's lines: the count, then each tool left out with its reason in plain words", () => {
  expect(importLines("en", { ...RESULT, imported: ["a", "b", "c"] })).toEqual([
    "Imported 3 tools.",
  ]);
  expect(
    importLines("en", {
      ...RESULT,
      imported: [],
      skipped: [
        { name: "filesystem", reason: "command_not_found", program: "npx" },
        { name: "github", reason: "unsupported_transport" },
        { name: "court-diary", reason: "unresolved_variable" },
        { name: "docket", reason: "already_added" },
        { name: "notes", reason: "unreadable" },
        { name: "matter-search", reason: "not_approved" },
      ],
    }),
  ).toEqual([
    "6 couldn't be imported.",
    "filesystem: Clauseo couldn't find npx on this computer.",
    "github: it uses a setting Clauseo doesn't support yet.",
    "court-diary: it needs a value that isn't set on this computer.",
    "docket: a tool with this name is already added.",
    "notes: Clauseo couldn't read its settings.",
    "matter-search: it comes from a project you haven't approved in Claude Code.",
  ]);
});

test("others: counted in the head, then one line after the named; alone, the head says it; nothing at all, no line", () => {
  const skipped: ImportResult["skipped"] = [{ name: "github", reason: "unsupported_transport" }];
  expect(importLines("en", { ...RESULT, imported: ["a"], skipped, others: 4 })).toEqual([
    "Imported 1 tool. 5 couldn't be imported.",
    "github: it uses a setting Clauseo doesn't support yet.",
    "4 more couldn't be imported.",
  ]);
  expect(importLines("en", { ...RESULT, imported: ["a", "b"], others: 3 })).toEqual([
    "Imported 2 tools. 3 couldn't be imported.",
  ]);
  expect(importLines("en", { ...RESULT, others: 0 })).toEqual([]);
});
