// What the room gives the tool line (tool-line.tsx), kept apart from it so the room's first paint carries none of the
// line: the plan's name, as the status line names it ("Claude"), Settings where the host has the window, and the
// transport whose Chrome download the line offers (browser-install.ts, which the line's chunk makes on first show).
import { createContext } from "react";
import type { HostTransport } from "../transport/host-transport.ts";

export type ToolLineHost = {
  plan: string;
  openSettings: (() => void) | undefined;
  transport: HostTransport | null;
};

export const ToolLineContext = createContext<ToolLineHost>({
  plan: "",
  openSettings: undefined,
  transport: null,
});
