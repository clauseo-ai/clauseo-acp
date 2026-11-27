// Add a skill from a link, a skills.sh name, a zip or a folder (RC A1, library L3; decision record
// 0040, "Adding from a link"): what the field holds, the host's skills.sh matches as the lawyer
// types, the preview (preview_skill), and Add (add_skills with the preview's token, the ticked
// skills by name and where they are on). One action at a time; a refusal stays in the panel with
// its code until the next action. Settings › Skills and the library's place both run it.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorSentence, ERROR_WORDS } from "@clauseo/components/lib/words-errors.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { useEffect, useRef, useState } from "react";
import type { AddonScope, CatalogueItem, SkillFrom, SkillPreview } from "../transport/bindings.ts";
import type { HostTransport } from "../transport/host-transport.ts";

/** What the field holds: a link, a skills.sh name, words to search skills.sh by, or nothing. */
export type FieldRead =
  | { kind: "from"; from: SkillFrom }
  | { kind: "words"; q: string }
  | { kind: "empty" };

const NAME = /^[\w.-]+\/[\w.-]+(@[\w./-]+)?(\/[\w.:-]+)?$/;

/** Reads the field: an https link (a bare github.com or skills.sh one gains https://), a skills.sh
 * name (owner/repo, owner/repo/skill, with an @ref), else words. */
export function readField(text: string): FieldRead {
  const t = text.trim();
  if (t === "") return { kind: "empty" };
  if (/^https?:\/\//i.test(t)) return { kind: "from", from: { url: t } };
  if (/^(www\.)?(github\.com|skills\.sh)\//i.test(t))
    return { kind: "from", from: { url: `https://${t}` } };
  if (NAME.test(t)) return { kind: "from", from: { skills_sh: t } };
  return { kind: "words", q: t };
}

/** The codes the panel says in their own words, with what it offers to mend each. */
export const ADD_CODES = [
  "skill.download_failed",
  "skill.not_found",
  "skill.too_big",
  "skill.preview_expired",
] as const;
type AddCode = (typeof ADD_CODES)[number];
const isAddCode = (code: string): code is AddCode =>
  (ADD_CODES as readonly string[]).includes(code);

/** Whether a failure is the way there, not the host's refusal: Add may have gone through. */
const unsure = (code: string) => code === "internal" || code.startsWith("transport.");

/** A refusal's words, and its one action: Try again for a failed download, Look again for an
 * expired preview. `adding`: it failed during Add, where a failure on the way may still have
 * added the skills, so the words send the lawyer to the list. */
export function troubleOf(
  lang: Lang,
  code: string,
  adding = false,
): { line: string; mend: "again" | "look" | null } {
  if (adding && unsure(code))
    return {
      line: lb(
        lang,
        "Clauseo couldn't tell whether that skill was added. Look for it in the list before you add it again.",
      ),
      mend: null,
    };
  if (code === "skill.no_skill_md")
    return {
      line: lb(lang, "There's no SKILL.md file in it. Choose a folder or a zip that holds one."),
      mend: null,
    };
  if (!isAddCode(code))
    return { line: lb(lang, "Clauseo couldn't add that skill, so nothing was added."), mend: null };
  const line = errorSentence(lang, ERROR_WORDS[code]);
  if (code === "skill.download_failed") return { line, mend: "again" };
  if (code === "skill.preview_expired") return { line, mend: "look" };
  return { line, mend: null };
}

const codeOf = (error: unknown): string =>
  typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : "internal";

/** Where the panel is: nothing asked, a preview being read, a preview shown, Add with the host,
 * or a refusal (with what it refused, for Try again). */
export type Phase =
  | { k: "idle" }
  | { k: "previewing"; from: SkillFrom }
  | { k: "preview"; from: SkillFrom; preview: SkillPreview }
  | { k: "adding"; from: SkillFrom; preview: SkillPreview }
  | {
      k: "failed";
      code: string;
      from: SkillFrom;
      preview: SkillPreview | null;
      /** It failed during Add, not the preview. */
      adding?: boolean;
    };

/** The preview the panel shows: the one read, kept under a refusal of Add. */
export function shownPreview(phase: Phase): SkillPreview | null {
  if (phase.k === "preview" || phase.k === "adding") return phase.preview;
  return phase.k === "failed" ? phase.preview : null;
}

export type AddSkill = {
  text: string;
  setText: (text: string) => void;
  /** skills.sh's matches for the words in the field, most relevant first. */
  matches: readonly CatalogueItem[];
  phase: Phase;
  /** The ticked skills, by name. */
  pick: ReadonlySet<string>;
  tick: (name: string, on: boolean) => void;
  scope: AddonScope;
  setScope: (scope: AddonScope) => void;
  /** Reads a skill source: the field's, a match's, or a picked file's. */
  look: (from: SkillFrom) => void;
  /** The native dialog: a zip (files) or a folder, then its preview. */
  choose: (files: boolean) => void;
  /** Sends add_skills for the ticked skills. */
  add: () => void;
  /** The refusal's action: the same preview again (Try again, Look again). */
  again: () => void;
  /** Back to the empty field. */
  reset: () => void;
};

const NONE = { q: "", items: [] };

/** How long the field waits after a key before it asks skills.sh. */
export const SEARCH_WAIT_MS = 300;

/** The flow over `transport`: `scope0` is where the skills start on; `added` runs after Add. */
export function useAddSkill(
  transport: HostTransport,
  scope0: AddonScope,
  added: () => void,
): AddSkill {
  const [text, setText] = useState("");
  // skills.sh's answer, with the words it answered: shown only while the field still holds them
  const [found, setFound] = useState<{ q: string; items: readonly CatalogueItem[] }>(NONE);
  const [phase, setPhase] = useState<Phase>({ k: "idle" });
  const [pick, setPick] = useState<ReadonlySet<string>>(new Set());
  const [scope, setScope] = useState<AddonScope>(scope0);
  // each ask's number: an answer counts only while no later ask was made
  const asked = useRef(0);
  const searched = useRef(0);
  const busy = phase.k === "previewing" || phase.k === "adding";

  const read = readField(text);
  const q = read.kind === "words" && read.q.length >= 2 ? read.q : null;
  const matches = q !== null && found.q === q ? found.items : [];
  useEffect(() => {
    const n = (searched.current += 1);
    if (q === null) return;
    const wait = setTimeout(() => {
      transport.query({ search_skills: { q } }).then(
        (answer) =>
          n === searched.current && setFound({ q, items: answer.skill_search?.items ?? [] }),
        // a search that fails shows no matches; a link or a name still previews
        () => n === searched.current && setFound(NONE),
      );
    }, SEARCH_WAIT_MS);
    return () => clearTimeout(wait);
  }, [q, transport]);

  const look = (from: SkillFrom, keep?: ReadonlySet<string>) => {
    const n = (asked.current += 1);
    searched.current += 1;
    setFound(NONE);
    setPhase({ k: "previewing", from });
    transport.query({ preview_skill: { from } }).then(
      (answer) => {
        if (n !== asked.current) return;
        const preview = answer.skill_preview;
        if (preview === undefined) {
          setPhase({ k: "failed", code: "internal", from, preview: null });
          return;
        }
        const names = preview.skills.map((s) => s.name);
        const kept = keep === undefined ? [] : names.filter((name) => keep.has(name));
        setPick(new Set(kept.length > 0 ? kept : names));
        setPhase({ k: "preview", from, preview });
      },
      (error: unknown) => {
        if (n === asked.current)
          setPhase({ k: "failed", code: codeOf(error), from, preview: null });
      },
    );
  };

  const reset = () => {
    asked.current += 1;
    setText("");
    setFound(NONE);
    setPick(new Set());
    setPhase({ k: "idle" });
  };

  return {
    text,
    setText: (next) => {
      setText(next);
      if (!busy && phase.k === "failed") setPhase({ k: "idle" });
    },
    matches,
    phase,
    pick,
    tick: (name, on) =>
      setPick((was) => {
        const next = new Set(was);
        if (on) next.add(name);
        else next.delete(name);
        return next;
      }),
    scope,
    setScope,
    look: (from) => !busy && look(from),
    choose: (files) => {
      if (busy || transport.pickSkill === undefined) return;
      void transport.pickSkill(files).then(
        (path) => path !== null && look({ path }),
        () => {},
      );
    },
    add: () => {
      const held = shownPreview(phase);
      if (held === null || busy || pick.size === 0) return;
      if (phase.k === "failed" && phase.code === "skill.preview_expired") return;
      const { from } = phase as { from: SkillFrom };
      const preview = held;
      const n = (asked.current += 1);
      // the order the preview found them in
      const names = preview.skills.map((s) => s.name).filter((name) => pick.has(name));
      setPhase({ k: "adding", from, preview });
      transport
        .dispatch({
          command_id: crypto.randomUUID(),
          command: { add_skills: { token: preview.token, pick: names, scope } },
        })
        .then(
          () => {
            if (n !== asked.current) return;
            reset();
            added();
          },
          (error: unknown) => {
            if (n === asked.current)
              setPhase({ k: "failed", code: codeOf(error), from, preview, adding: true });
          },
        );
    },
    again: () => {
      if (phase.k === "failed") look(phase.from, pick);
    },
    reset,
  };
}
