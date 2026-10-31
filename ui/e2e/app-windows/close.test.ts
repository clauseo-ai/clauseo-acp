// The close script's text and output parsing, on any OS; closeWindow itself needs Windows.

import { expect, test } from "@playwright/test";
import { closeScript, parsePosted, WINDOW_CLASS } from "./close.ts";

test("the script posts WM_CLOSE to the Tauri windows the PID owns, and prints the count", () => {
  const script = closeScript(4242);
  expect(script).toContain(`'${WINDOW_CLASS}'`);
  expect(script).toContain("if ($owner -ne 4242) { continue }");
  expect(script).toContain("PostMessage($hwnd, 16,");
  // PowerShell passes $null to a string parameter as "", which matches only untitled windows; the
  // app's window is titled, so the title must go as a real null.
  expect(script).toContain("[NullString]::Value)");
  expect(script).not.toContain("$null");
  expect(script.trimEnd().endsWith("$posted")).toBe(true);
  // The here-string's markers must each stand on a line of their own.
  expect(script).toMatch(/@'\n/);
  expect(script).toMatch(/\n'@\n/);
});

test("the script refuses anything but a positive integer PID", () => {
  for (const pid of [0, -1, 1.5, Number.NaN]) {
    expect(() => closeScript(pid)).toThrow(/not a PID/);
  }
});

test("parsePosted reads the count and refuses other output", () => {
  expect(parsePosted("1\r\n")).toBe(1);
  expect(parsePosted("0\n")).toBe(0);
  expect(() => parsePosted("")).toThrow(/not a window count/);
  expect(() => parsePosted("Exception: boom")).toThrow(/not a window count/);
});
