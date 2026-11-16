// The Windows line's notice in the room (UC-820), once: on Windows, the first time the room lists a second matter (the
// Scratch matter counts: its files are a matter's like any other), until the lawyer presses "Got it" (kept.ts). On the
// Mac and Linux, or with one matter, nothing. The notice itself is its own chunk (windows-line-note.tsx), loaded only
// when it shows, inside the room's Suspense beside the Smart App Control note.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import { lazy, useState } from "react";
import { markWindowsLineSeen, windowsLineSeen } from "./kept.ts";

const WindowsLineNote = lazy(() =>
  late(import("./windows-line-note.tsx")).then((m) => ({ default: m.WindowsLineNote })),
);

/** Whether the notice shows, for `matters` matters on `platform`, put away before or not. */
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
