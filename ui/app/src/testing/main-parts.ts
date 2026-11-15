// Main's command splitter as it stood at b0bb3f3a2, before the sh and PowerShell readings: the floor the property
// test in room/thread/task-words.test.ts holds every new count to.

/** Main's partsOf, unchanged: split outside quotes, never at `2>&1`'s lone `&`; a backslash escapes nothing but
 * find's `\;` as a word of its own. */
export function mainPartsOf(command: string): string[] {
  const parts: string[] = [];
  let quote: string | null = null;
  let from = 0;
  for (let i = 0; i < command.length; i++) {
    const c = command[i]!;
    if (quote !== null) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === "\\" && command[i + 1] === ";" && (i === 0 || /\s/.test(command[i - 1]!)))
      i += 1;
    else if (c === ";" || c === "|" || (c === "&" && command[i + 1] === "&")) {
      parts.push(command.slice(from, i));
      if (c !== ";" && command[i + 1] === c) i += 1;
      from = i + 1;
    }
  }
  parts.push(command.slice(from));
  return parts;
}
