// Every words file's Hindi, as one chunk: the room and the settings window load it only for a Hindi display
// (words-core.ts's loadHindi, which display.ts calls), so an English page carries none of it. Importing this file
// fills the registry at once, as the tests, the copy review and the gallery do.
import { registerHindi } from "./words-core.ts";
import { HI as breaks } from "./words-breaks.hi.ts";
import { HI as connectors } from "./words-connectors.hi.ts";
import { HI as connectorsRoom } from "./words-connectors-room.hi.ts";
import { HI as errors } from "./words-errors.hi.ts";
import { HI as find } from "./words-find.hi.ts";
import { HI as firstLaunch } from "./words-first-launch.hi.ts";
import { HI as firstRead } from "./words-first-read.hi.ts";
import { HI as folderCard } from "./words-folder-card.hi.ts";
import { HI as folder } from "./words-folder.hi.ts";
import { HI as frame } from "./words-frame.hi.ts";
import { HI as limitsThread } from "./words-limits-thread.hi.ts";
import { HI as limits } from "./words-limits.hi.ts";
import { HI as list } from "./words-list.hi.ts";
import { HI as matterForm } from "./words-matter-form.hi.ts";
import { HI as matters } from "./words-matters.hi.ts";
import { HI as planCard } from "./words-plan-card.hi.ts";
import { HI as reader } from "./words-reader.hi.ts";
import { HI as room } from "./words-room.hi.ts";
import { HI as scan } from "./words-scan.hi.ts";
import { HI as settings } from "./words-settings.hi.ts";
import { HI as signinExpired } from "./words-signin-expired.hi.ts";
import { HI as sources } from "./words-sources.hi.ts";
import { HI as startup } from "./words-startup.hi.ts";
import { HI as stepsLive } from "./words-steps-live.hi.ts";
import { HI as steps } from "./words-steps.hi.ts";
import { HI as thread } from "./words-thread.hi.ts";
import { HI as toolbar } from "./words-toolbar.hi.ts";
import { HI as toolResult } from "./words-tool-result.hi.ts";
import { HI as toolsLine } from "./words-tools-line.hi.ts";
import { HI as tools } from "./words-tools.hi.ts";
import { HI as triageLate } from "./words-triage-late.hi.ts";
import { HI as triage } from "./words-triage.hi.ts";
import { HI as ui } from "./words-ui.hi.ts";
import { HI as versions } from "./words-versions.hi.ts";
import { HI as words } from "./words.hi.ts";

/** Each words file's Hindi by its area, the file's name less "words-": wordsOf("errors") reads words-errors.hi.ts. */
export const HINDI = {
  breaks,
  connectors,
  "connectors-room": connectorsRoom,
  errors,
  find,
  "first-launch": firstLaunch,
  "first-read": firstRead,
  "folder-card": folderCard,
  folder,
  frame,
  "limits-thread": limitsThread,
  limits,
  list,
  "matter-form": matterForm,
  matters,
  "plan-card": planCard,
  reader,
  room,
  scan,
  settings,
  "signin-expired": signinExpired,
  sources,
  startup,
  "steps-live": stepsLive,
  steps,
  thread,
  toolbar,
  "tool-result": toolResult,
  "tools-line": toolsLine,
  tools,
  "triage-late": triageLate,
  triage,
  ui,
  versions,
  words,
} as const;

registerHindi(HINDI);
