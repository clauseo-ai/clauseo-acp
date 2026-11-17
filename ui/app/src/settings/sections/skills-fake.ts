// A fake host for Settings › Skills, for its tests and the copy review: the packs `list_skill_packs` lists, what
// `add_skill`, `remove_skill` and `set_skill_pack` do to them, the picker's answer, and a refusal for the next command.
// Never shipped.
import type {
  CommandEnvelope,
  CommandOutcome,
  HostQuery,
  QueryResult,
} from "../../transport/protocol.ts";
import type { SkillCommand, SkillPack } from "./skills-contract.ts";

/** A pack as the host lists it: on, unless `more` says otherwise. */
export const pack = (id: string, name: string, more: Partial<SkillPack> = {}): SkillPack => ({
  id,
  name,
  state: "on",
  ...more,
});

/** The packs the shots and tests start from: one added here, one from an address, one changed and one unusable. */
export const PACKS: SkillPack[] = [
  pack("notice-drafting", "Notice drafting", { added_here: true }),
  pack("cheque-bounce", "Cheque bounce replies", {
    state: "off",
    source_url: "https://github.com/lexindia/skills",
  }),
  pack("lease-review", "Lease review", { state: "changed", added_here: true }),
  pack("old-briefs", "Old briefs", {
    state: "invalid",
    reason: { invalid: { kind: "script" } },
    added_here: true,
  }),
];

export class FakeSkills {
  packs: SkillPack[];
  /** What the picker answers next: a path, or null for Cancel. */
  pick: string | null = "/Users/r/Skills/bail-applications";
  /** The next command's refusal, then none. */
  refuseNext: string | null = null;
  /** Every skill command sent, in order. */
  readonly sent: SkillCommand[] = [];

  constructor(packs: readonly SkillPack[] = []) {
    this.packs = packs.map((p) => ({ ...p }));
  }

  /** The FakeTransport host's parts: the query, the commands and the picker. */
  host() {
    return {
      query: (query: HostQuery): QueryResult => {
        if (query === "list_skill_packs") return { skill_packs: this.packs.map((p) => ({ ...p })) };
        throw new Error("not scripted");
      },
      dispatch: (envelope: CommandEnvelope): CommandOutcome => {
        const command = envelope.command as unknown as SkillCommand;
        if (!("add_skill" in command || "remove_skill" in command || "set_skill_pack" in command))
          return "done";
        this.sent.push(command);
        const refusal = this.refuseNext;
        if (refusal !== null) {
          this.refuseNext = null;
          throw { code: refusal, message: "refused", retryable: false };
        }
        if ("add_skill" in command) {
          const id = command.add_skill.path.split(/[\\/]/).pop() ?? "skill";
          this.packs.push(pack(id, id, { state: "off", added_here: true }));
        } else if ("remove_skill" in command)
          this.packs = this.packs.filter((p) => p.id !== command.remove_skill.id);
        else {
          const { id, on } = command.set_skill_pack;
          this.packs = this.packs.map((p) =>
            p.id === id ? { ...p, state: on ? "on" : "off" } : p,
          );
        }
        return "done";
      },
      pickSkill: () => this.pick,
    };
  }
}
