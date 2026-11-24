-- Schema version 8: a matter's own switches for skills and tools (decision record 0040, Scope and
-- per-matter switches). docs/decisions/0040-skill-packs.md gives the reason and what NULL means.
-- Frozen: a later change is a new numbered file in this folder, never an edit here.

ALTER TABLE matters ADD COLUMN addon_switches TEXT /* JSON {"skill":{id:bool},"tool":{name:bool}}; NULL = no switch, each item follows its scope */;
