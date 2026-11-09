// Shared by the tool line's tests: the room a row is read in, the words a row gives for a tool, and the notes that
// say in full why the firm's policy, an unreadable policy, or a Chrome version that can't be checked blocks a browser.
import { toolRow, type ToolRowRoom } from "./tool-rows.ts";
import type { KnownTool } from "./tools.ts";

export const POLICY =
  "Your firm doesn't allow apps to control a browser on this computer, so Clauseo can't use one for you.";
export const UNREADABLE =
  "Clauseo couldn't check this computer's browser settings, so it won't use a browser for now. " +
  "It checks again the next time it needs one.";
export const NO_LIST =
  "Clauseo couldn't confirm that its copy of Chrome is up to date, so it isn't using the browser for " +
  "now. It keeps checking on its own.";
export const LIST_EXPIRED =
  "Clauseo's copy of Chrome is too old to use, and Clauseo can't update it right now. " +
  "It keeps trying on its own.";

export const ROOM: ToolRowRoom = { settings: true, install: null };
export const words = (tool: KnownTool, room: Partial<ToolRowRoom> = {}) =>
  toolRow("en", tool, { ...ROOM, ...room });
