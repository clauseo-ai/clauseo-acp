// Settings › Skills' seam to the host: `list_skill_packs` read as the section opens, after each command and on each
// `settings_changed`; `set_skill_pack`, `add_skill` and `remove_skill` sent from here alone; and the native picker
// (`pickSkill`, the desktop's `pick_skill`). An answer stands only while no later read was asked for. A refusal comes
// back as its code.
import { useEffect, useState, useSyncExternalStore } from "react";
import { nextHostOf } from "../../room/host-next.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { HostCommand, PackStatus } from "../../transport/protocol.ts";

/** What Settings › Skills sends. */
type SkillCommand = Extract<
  HostCommand,
  { set_skill_pack: unknown } | { add_skill: unknown } | { remove_skill: unknown }
>;

const codeOf = (error: unknown): string =>
  typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : "internal";

export type SkillsState = {
  /** The packs as last read; null before the first answer. */
  packs: readonly PackStatus[] | null;
  /** The code of the last command the host refused; cleared by the next that goes through. */
  failed: string | null;
  /** A command is with the host (the picker included). */
  busy: boolean;
};

const FIRST: SkillsState = { packs: null, failed: null, busy: false };

function skillsOver(transport: HostTransport) {
  let state = FIRST;
  let asked = 0;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<SkillsState>) => {
    state = { ...state, ...patch };
    for (const listener of listeners) listener();
  };
  const read = async (): Promise<void> => {
    const n = (asked += 1);
    try {
      const answer = await transport.query("list_skill_packs");
      if (n === asked) set({ packs: answer.skill_packs ?? [] });
    } catch {
      // the list stays as last read; a first read that fails shows the empty list
      if (n === asked && state.packs === null) set({ packs: [] });
    }
  };
  /** One command (none when the picker was cancelled), then the list again. */
  const run = async (command: () => Promise<SkillCommand | null>) => {
    if (state.busy) return;
    set({ busy: true });
    let failed: string | null = null;
    try {
      const sent = await command();
      if (sent === null) return;
      await transport
        .dispatch({ command_id: crypto.randomUUID(), command: sent })
        .catch((error: unknown) => (failed = codeOf(error)));
      await read();
    } catch (error: unknown) {
      failed = codeOf(error);
    } finally {
      set({ busy: false, failed });
    }
  };
  return {
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    state: () => state,
    read,
    add: () =>
      run(async () => {
        const path = (await transport.pickSkill?.()) ?? null;
        return path === null ? null : { add_skill: { path } };
      }),
    remove: (id: string) => run(async () => ({ remove_skill: { id } })),
    turn: (id: string, on: boolean) => run(async () => ({ set_skill_pack: { id, on } })),
  };
}

export type Skills = SkillsState & {
  add: () => void;
  /** The list read again: after Add a skill's add_skills. */
  read: () => void;
  remove: (id: string) => void;
  turn: (id: string, on: boolean) => void;
};

/** The packs, read as the section opens and on each `settings_changed`, with the section's commands. */
export function useSkills(transport: HostTransport): Skills {
  const [skills] = useState(() => skillsOver(transport));
  const state = useSyncExternalStore(skills.subscribe, skills.state, skills.state);
  useEffect(() => {
    void skills.read();
    return nextHostOf(transport).settings?.onSettingsChanged(() => void skills.read());
  }, [skills, transport]);
  return {
    ...state,
    add: () => void skills.add(),
    read: () => void skills.read(),
    remove: (id) => void skills.remove(id),
    turn: (id, on) => void skills.turn(id, on),
  };
}
