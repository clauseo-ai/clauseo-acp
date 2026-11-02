// What the hostile slice files share: hostile-stream-<k>.test.tsx (hostile-dom-fixture.tsx) and
// hostile-time-<k>.timed.test.ts (hostile-time-fixture.ts) each take slice k, k from the file's own name.
import { expect } from "vitest";

/**
 * The <k> in the name of the test file being collected, `<prefix><k><suffix>`, between 1 and `files`: a slice file
 * takes its slice from its own name, so it cannot ask for another file's, and no file can be missing or doubled
 * without the guard seeing it (hostile-all.test.tsx, hostile-blocks.test.ts).
 */
export function sliceOfThisFile(prefix: string, suffix: string, files: number): number {
  const name = (expect.getState().testPath ?? "").split(/[\\/]/).pop() ?? "";
  const k =
    name.startsWith(prefix) && name.endsWith(suffix)
      ? name.slice(prefix.length, -suffix.length)
      : "";
  if (!/^[1-9]\d*$/.test(k) || Number(k) > files)
    throw new Error(`${name} is not ${prefix}<1 to ${files}>${suffix}`);
  return Number(k);
}

/**
 * Whether a slice file's `source` takes its slice whole: it calls `helper` once, with nothing, and has no
 * `.slice(`, `.filter(` or other call of the slice functions that could narrow it.
 */
export function takesWholeSlice(source: string, helper: string, sliceFunction: string): boolean {
  const code = source.replace(/\/\/.*$/gm, "");
  return (
    code.split(`${helper}(`).length === 2 &&
    code.includes(`${helper}();`) &&
    !/\.(?:slice|filter|splice|shift|pop)\(/.test(code) &&
    !code.includes(sliceFunction)
  );
}
