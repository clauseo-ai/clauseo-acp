-- Schema version 5: a matter's skill packs (decision record 0040).
-- docs/decisions/0040-skill-packs.md gives the reason for the column and what NULL means.
-- Frozen: a later change is a new numbered file in this folder, never an edit here.

ALTER TABLE matters ADD COLUMN skill_packs TEXT /* JSON array of pack ids, at most 32, none twice; NULL = the global skills.enabled list */;
