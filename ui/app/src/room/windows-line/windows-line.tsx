// The Windows line's notice in the room (UC-820, then UC-850): on Windows, from the first time the room lists a second
// matter (Scratch counts; windowsLineDue says why), at the top of every conversation until the lawyer presses "Got
// it" (kept.ts). Settings › Privacy keeps the same words readable on Windows. On the
// Mac and Linux, or with one matter, nothing. The notice itself is its own chunk (windows-line-note.tsx), loaded only
// when it shows, inside the room's Suspense beside the Smart App Control note.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import { lazy, useState } from "react";
import { markWindowsLineSeen, windowsLineSeen } from "./kept.ts";

const WindowsLineNote = lazy(() =>
  late(import("./windows-line-note.tsx")).then((m) => ({ default: m.WindowsLineNote })),
);

/** Whether the notice shows, for `matters` matters on `platform`, put away before or not. The Scratch matter counts
 * toward the two on purpose: a conversation in Scratch can reach the lawyer's first folder matter in the ways the line
 * names (a program it writes and runs, a pieced-together location, a shortcut), so Scratch and one folder matter
 * already make two matters to keep apart. */
const windowsLineDue = (platform: string, matters: number, seen: boolean): boolean =>
  platform === "windows" && matters >= 2 && !seen;

export function WindowsLine({ matters }: { matters: number }) {
  const { platform } = useDisplay();
  const [seen, setSeen] = useState(() => windowsLineSeen() !== null);
  if (!windowsLineDue(platform, matters, seen)) return null;
  return (
    <WindowsLineNote
      onGotIt={() => {
        markWindowsLineSeen();
        setSeen(true);
      }}
    />
  );
}
