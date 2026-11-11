// UC's copy review entries from UC-80 to UC-106, apart from registry-uc.tsx to keep that file under the size limit:
// design B's blocks and their repair (UC-80 to UC-93 and UC-96), the Add room confirms after the repair (UC-94 and
// UC-95), and UC-106, the line for a host that runs Claude on its own in the Safe workspace (not shown in the app yet).
// They use registry-uc.tsx's helpers and words. UC_BLOCKS follows UC in registry-uc-blackline.tsx's UC_ALL.
import { SafeWorkspaceLine } from "@clauseo/components/clauseo/safe-workspace-line.tsx";
import type { Entry } from "./entry.ts";
import {
  first,
  held,
  LINE,
  NO_PROMPT,
  note,
  P,
  pw,
  room,
  SETUP,
  settings,
  viewOf,
} from "./registry-uc.tsx";
import { roomWords } from "./words.ts";

/** UC's entries, UC-80 to UC-106, each id written as it is; they follow registry-uc.tsx's UC in the list. */
export const UC_BLOCKS: Entry[] = [
  // design B: what stops the Safe workspace, and the repair
  room(
    "UC-80",
    pw(
      "Something on this computer stops the Safe workspace, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.",
    ),
    "room/np-note",
    note(P.policy),
  ),
  settings(
    "UC-81",
    pw(
      "Something on this computer stops the Safe workspace, so Clauseo asks before changing anything outside a matter's folder.",
    ),
    held(P.policy),
  ),
  settings(
    "UC-82",
    pw("A Windows policy on this computer blocks the program that sets up the Safe workspace."),
    held(P.policy),
  ),
  room(
    "UC-83",
    pw(
      "The program that sets up the Safe workspace is missing, or security software blocked it. Allow Clauseo in your security software, then install Clauseo again.",
    ),
    "room/np-note",
    note(P.tool),
  ),
  first(
    "UC-84",
    pw("The Safe workspace wasn't set up when Clauseo was installed."),
    undefined,
    {},
    P.missing,
  ),
  first(
    "UC-85",
    pw("Run the Clauseo installer again to set up the Safe workspace."),
    undefined,
    {},
    P.missing,
  ),
  first("UC-86", pw("Get the installer"), undefined, {}, P.missing),
  settings(
    "UC-87",
    pw("Your firm manages how Clauseo is installed on this computer."),
    held(P.missing, { required: true }),
  ),
  settings(
    "UC-88",
    pw(
      "The program that sets up the Safe workspace has changed since Clauseo was installed. Install Clauseo again to restore it.",
    ),
    held(P.changed),
  ),
  settings(
    "UC-89",
    pw(
      "The folder that holds the Safe workspace's set-up program isn't limited to administrators, or holds files it shouldn't. Install Clauseo again to restore it.",
    ),
    held(P.unsafe),
  ),
  room(
    "UC-90",
    pw(
      "The Safe workspace on this computer belongs to another Windows account, so Clauseo can't use it from this one.",
    ),
    "room/np-note",
    note(P.other),
  ),
  settings("UC-91", pw("Something on this computer stops the Safe workspace."), held(P.newer)),
  room(
    "UC-92",
    pw(
      "Your firm requires the Safe workspace on this computer, so Clauseo can't answer here until it's set up.",
    ),
    "room/protection-required",
    { protection: P.missing, over: { required: true } },
  ),
  settings(
    "UC-93",
    pw(
      "Something on this computer stops the Safe workspace, so Clauseo can't answer here. Settings › Safe workspace says what to do.",
    ),
    {
      view: () => viewOf(P.failed, NO_PROMPT),
      command: () => Promise.reject({ code: "sandbox.blocked", message: "", retryable: false }),
    },
    [SETUP],
  ),
  room("UC-94", pw("Add room for 4 more?"), "room/protection-confirm", {
    protection: P.pool,
    press: [LINE, SETUP],
  }),
  room(
    "UC-95",
    pw(
      "Windows will ask to let Clauseo add room to the Safe workspace. It will say the publisher is unknown. That's expected for Clauseo. Choose Yes. If Windows asks for an administrator's password, enter the password of an administrator account on this computer.",
    ),
    "room/protection-confirm",
    { protection: P.pool, press: [LINE, SETUP] },
  ),
  first(
    "UC-96",
    pw(
      "Clauseo will close so the installer can set up the Safe workspace. Run the installer you download, then open Clauseo again.",
    ),
    undefined,
    {},
    P.missing,
  ),
  // UC-01 once the host runs Claude on its own in the Safe workspace (D2d): not shown in the app until then
  {
    id: "UC-106",
    words: roomWords(
      "Safe workspace. Clauseo changes files in this matter without asking. It can't open your other files unless you allow it.",
    ),
    screen: "room/protection-line",
    render: () => (
      <p className="room-line">
        <SafeWorkspaceLine worksOnItsOwn />
      </p>
    ),
  },
];
