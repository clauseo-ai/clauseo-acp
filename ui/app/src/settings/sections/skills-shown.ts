// Whether Settings › Skills shows: where the transport has the native skill picker (the desktop's `pick_skill`). Apart
// from skills-commands.ts, so the settings window's first load carries none of the section.
import type { HostTransport } from "../../transport/host-transport.ts";

export const skillsShown = (transport: HostTransport): boolean => transport.pickSkill !== undefined;
