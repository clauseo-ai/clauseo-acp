// A scripted host for Add a skill (RC A1), for its tests and the copy review: skills.sh's matches by
// the words searched, a preview for each source, a refusal for the next preview or Add, a gate that
// holds either open, and what Add sent. Never shipped.
import type {
  CatalogueItem,
  CommandEnvelope,
  CommandOutcome,
  HostCommand,
  HostQuery,
  PreviewedSkill,
  QueryResult,
  SkillFrom,
  SkillPreview,
} from "../transport/bindings.ts";

const skill = (name: string, description: string, more: Partial<PreviewedSkill> = {}) => ({
  name,
  description,
  has_scripts: false,
  already_added: false,
  ...more,
});

/** lexdesk/skills: three skills, one already added, one with programs. */
export const LEXDESK: Omit<SkillPreview, "token"> = {
  from: { kind: "skills_sh", text: "lexdesk/skills" },
  skills: [
    skill("bail-applications", "Drafts a bail application from the FIR and the charge sheet."),
    skill("cheque-bounce", "Replies to a notice under section 138 of the NI Act.", {
      already_added: true,
    }),
    skill("court-fees", "Works out court fees by the state's schedule.", { has_scripts: true }),
  ],
};

/** A zip of one skill. */
export const ZIPPED: Omit<SkillPreview, "token"> = {
  from: { kind: "zip", text: "lease-review.zip" },
  skills: [skill("lease-review", "Reads a lease and lists the clauses to negotiate.")],
};

export const MATCHES: CatalogueItem[] = [
  {
    id: "lexdesk/skills/contract-review",
    name: "contract-review",
    source: "lexdesk/skills",
    installs: 1_204,
    page: "https://skills.sh/lexdesk/skills/contract-review",
  },
  {
    id: "anthropics/skills/docx",
    name: "docx",
    source: "anthropics/skills",
    installs: 98_120,
    page: "https://skills.sh/anthropics/skills/docx",
  },
];

type Step = "preview" | "add";

export class FakeSkillLinks {
  /** The preview each source gives, by its JSON; any other source previews as lexdesk/skills. */
  previews = new Map<string, Omit<SkillPreview, "token">>();
  /** The next refusal of each step, then none. */
  refuse: Partial<Record<Step, string>> = {};
  /** While set, the step waits for it. */
  gate: Partial<Record<Step, Promise<void>>> = {};
  /** Every search, preview and Add, in order. */
  readonly asked: (HostQuery | HostCommand)[] = [];
  /** The names each Add added. */
  readonly added: string[] = [];
  #tokens = 0;

  #step = async (step: Step) => {
    const gate = this.gate[step];
    if (gate !== undefined) await gate;
    const code = this.refuse[step];
    if (code !== undefined) {
      delete this.refuse[step];
      throw { code, message: "refused", retryable: false };
    }
  };

  /** Answers search_skills and preview_skill; null for any other query. */
  async query(query: HostQuery): Promise<QueryResult | null> {
    if (typeof query !== "object") return null;
    if ("search_skills" in query) {
      this.asked.push(query);
      const words = query.search_skills.q.toLowerCase();
      return { skill_search: { items: MATCHES.filter((m) => m.id.includes(words)) } };
    }
    if ("preview_skill" in query) {
      this.asked.push(query);
      await this.#step("preview");
      const from: SkillFrom = query.preview_skill.from;
      const preview = this.previews.get(JSON.stringify(from)) ?? LEXDESK;
      this.#tokens += 1;
      return { skill_preview: { token: `t${this.#tokens}`, ...preview } };
    }
    return null;
  }

  /** Answers add_skills; null for any other command. */
  async dispatch(envelope: CommandEnvelope): Promise<CommandOutcome | null> {
    const command = envelope.command;
    if (!("add_skills" in command) || command.add_skills === undefined) return null;
    this.asked.push(command);
    await this.#step("add");
    this.added.push(...command.add_skills.pick);
    return "done";
  }
}
