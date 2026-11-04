// The holders script and its parsing, on any OS; finding and killing real processes on Windows.

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as wait } from "node:timers/promises";
import { expect, test } from "@playwright/test";
import { findHolders, holdersScript, killHolders, parseHolders, WEBVIEW_IMAGE } from "./holders.ts";

test("the script lists one image's processes with their command lines, always as an array", () => {
  const script = holdersScript();
  expect(script).toContain(`-Filter "Name='${WEBVIEW_IMAGE}'"`);
  expect(script).toContain("Select-Object ProcessId, CommandLine");
  // -InputObject keeps one process, or none, an array; piped, ConvertTo-Json unrolls it.
  expect(script).toContain("ConvertTo-Json -Compress -InputObject $found");
  expect(holdersScript("node.exe")).toContain(`"Name='node.exe'"`);
});

test("the script refuses an image name that could break out of its filter", () => {
  for (const image of ["", "a'b.exe", 'a"b.exe', "a b.exe", "a;b"]) {
    expect(() => holdersScript(image)).toThrow(/not a process image name/);
  }
});

test("parseHolders keeps the processes whose command line names the root, without case", () => {
  const root = String.raw`C:\Users\ci\AppData\Local\Temp\clauseo-e2e-AbC123`;
  const rows = [
    {
      ProcessId: 11,
      CommandLine: String.raw`"msedgewebview2.exe" --type=browser --user-data-dir="C:\Users\ci\AppData\Local\Temp\clauseo-e2e-AbC123\webview-1\EBWebView" --x`,
    },
    {
      ProcessId: 12,
      CommandLine: String.raw`msedgewebview2.exe --type=renderer --user-data-dir=c:\users\ci\appdata\local\temp\CLAUSEO-E2E-abc123\webview-2\EBWebView`,
    },
    // Another test's folder, whose name only starts like this one's.
    {
      ProcessId: 13,
      CommandLine: String.raw`msedgewebview2.exe --user-data-dir="C:\Users\ci\AppData\Local\Temp\clauseo-e2e-AbC1234\webview-1"`,
    },
    // Another app's WebView2, and a process whose command line Windows does not show.
    {
      ProcessId: 14,
      CommandLine: String.raw`msedgewebview2.exe --user-data-dir="C:\Other\EBWebView"`,
    },
    { ProcessId: 15, CommandLine: null },
  ];
  expect(parseHolders(JSON.stringify(rows), root)).toEqual([11, 12]);
  expect(parseHolders(JSON.stringify(rows), `${root}\\`)).toEqual([11, 12]);
  expect(parseHolders("[]", root)).toEqual([]);
});

test("parseHolders refuses output that is not the script's", () => {
  expect(() => parseHolders("", "C:\\t")).toThrow(/not JSON/);
  expect(() => parseHolders("Get-CimInstance : Invalid query", "C:\\t")).toThrow(/not JSON/);
  expect(() => parseHolders('{"ProcessId":1,"CommandLine":"x"}', "C:\\t")).toThrow(
    /unexpected shape/,
  );
  expect(() => parseHolders('[{"ProcessId":"1","CommandLine":"x"}]', "C:\\t")).toThrow(
    /unexpected shape/,
  );
});

test("on Windows: a process naming the root is found, killed, and then no longer found", async () => {
  test.skip(process.platform !== "win32", "finding and killing processes needs Windows");
  const root = mkdtempSync(join(tmpdir(), "clauseo-holders-test-"));
  // node.exe stands in for msedgewebview2.exe, with the folder on its command line as WebView2's is.
  const child = spawn(
    process.execPath,
    ["-e", "setInterval(() => {}, 1000)", join(root, "webview-1")],
    {
      stdio: "ignore",
    },
  );
  const exited = new Promise((resolve) => child.once("exit", resolve));
  try {
    const pid = child.pid;
    expect(pid).toBeDefined();
    let found: number[] = [];
    for (let i = 0; i < 20 && !found.includes(pid!); i++) {
      found = await findHolders(root, "node.exe");
      if (!found.includes(pid!)) await wait(500);
    }
    expect(found).toEqual([pid]);
    await killHolders(found);
    await exited;
    expect(await findHolders(root, "node.exe")).toEqual([]);
    // A PID already gone is no error.
    await killHolders(found);
  } finally {
    child.kill();
    rmSync(root, { recursive: true, force: true });
  }
});
