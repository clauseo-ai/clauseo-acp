// A step that loads a skill, as the lawyer reads it: reading a guide. Claude's Skill tool comes titled "Load skill:
// clauseo:legal-drafting" (claude-agent-acp's SkillReporter), its input `{ skill }`; ChatGPT reads a skill's SKILL.md
// itself, by a read or a shell command naming the file. Neither sends a title for people, and a skill's manifest
// declares only its name and a description written for the model, so Clauseo's own skills are named here
// ("Clauseo's drafting guide") and any other is "a guide". The internal name ("clauseo:legal-drafting") is never
// shown. In the room's first chunk with the working line: the guides' names are words-steps-live.ts's.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { stepLive, type StepLiveKey } from "@clauseo/components/lib/words-steps-live.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";

/** Clauseo's own skills (crates/profile/assets/skills), by their name without the `clauseo:` namespace. */
const GUIDES: Readonly<Record<string, StepLiveKey>> = {
  "legal-drafting": "Clauseo's drafting guide",
  "legal-research": "Clauseo's research guide",
  "matter-brief": "Clauseo's matter brief guide",
  "agent-browser": "Clauseo's browser guide",
};

const LOAD = /^\s*`?Load skill\b:?\s*`?([^`]*)`?\s*$/i;
/** A skill's SKILL.md in a path or a command: its folder is the skill's name. */
const SKILL_FILE = /(?:^|[/\\])([A-Za-z0-9_.-]+)[/\\]SKILL\.md\b/i;
const RAW_MAX = 16_384;

function rawOf(entry: ToolCallEntry): Record<string, unknown> | null {
  const text = entry.raw_input?.inline;
  if (text === undefined || text.length > RAW_MAX || !text.startsWith("{")) return null;
  try {
    const v: unknown = JSON.parse(text);
    return typeof v === "object" && v !== null && !Array.isArray(v)
      ? (v as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

/** A skill's name with its namespace and any arguments off: "clauseo:legal-drafting args" → "legal-drafting". */
const bare = (name: string): string =>
  (name.trim().split(/\s+/)[0] ?? "").replace(/^.*:/, "").toLowerCase();

/** A title that loads a skill ("Load skill: clauseo:legal-drafting"): its bare name, "" with none; null otherwise. */
export function skillTitle(title: string): { name: string } | null {
  const loaded = LOAD.exec(title);
  return loaded === null ? null : { name: bare(loaded[1] ?? "") };
}

/** What a skill step names: `name` its bare name, "" when none is given. Null when the call loads no skill. */
export function skillOf(entry: ToolCallEntry): { name: string } | null {
  const raw = rawOf(entry);
  const loaded = LOAD.exec(entry.title);
  if (entry.tool_name?.toLowerCase() === "skill" || loaded !== null) {
    const given = typeof raw?.["skill"] === "string" ? raw["skill"] : (loaded?.[1] ?? "");
    return { name: bare(given) };
  }
  // a skill's file, read as it is or by a command (ChatGPT's way)
  if (entry.kind !== "read" && entry.kind !== "execute") return null;
  const places: string[] = [entry.title];
  for (const l of entry.locations)
    if (typeof l === "object") places.push(l.inside?.path ?? l.outside?.display ?? "");
  for (const k of ["file_path", "path", "command", "cmd"]) {
    const v = raw?.[k];
    if (typeof v === "string") places.push(v);
    else if (Array.isArray(v)) places.push(v.filter((a) => typeof a === "string").join(" "));
  }
  for (const p of places) {
    const m = SKILL_FILE.exec(p);
    if (m !== null) return { name: bare(m[1]!) };
  }
  return null;
}

/** The guide's name in the language ("Clauseo's drafting guide"), or null for a skill Clauseo doesn't ship. */
export function guideOf(lang: Lang, skill: { name: string }): string | null {
  const key = GUIDES[skill.name];
  return key === undefined ? null : stepLive(lang, key);
}
