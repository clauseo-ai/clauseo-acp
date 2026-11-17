// Settings › Skills' wire, written by hand until the host's "Add a skill" lands in the generated bindings
// (crates/protocol/src/skill_packs.rs): the pack as `list_skill_packs` lists it, with `added_here`, and the commands
// `add_skill` and `remove_skill`, shaped as `set_skill_pack` is. When the bindings carry them, this file becomes
// re-exports of theirs and nothing else moves.
import type { HostCommand, PackStatus } from "../../transport/protocol.ts";

/** One pack as `list_skill_packs` lists it, with whether the lawyer added it on this computer (`add_skill`). */
export type SkillPack = PackStatus & { added_here?: boolean };

/** What Settings › Skills sends: `set_skill_pack`, `add_skill` (a folder or a SKILL.md file) and `remove_skill`. */
export type SkillCommand =
  | Extract<HostCommand, { set_skill_pack: unknown }>
  | { add_skill: { path: string } }
  | { remove_skill: { id: string } };

/** The one refusal the section names; any other gets the generic line. */
export const NO_SKILL_MD = "skill.no_skill_md";
