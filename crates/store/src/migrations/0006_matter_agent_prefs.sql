-- Schema version 6: a matter's model and effort picks per agent kind (UpdateMatter's agent_prefs).
-- docs/decisions/0006-store-format.md gives the reason for the column and what NULL means.
-- Frozen: a later change is a new numbered file in this folder, never an edit here.

ALTER TABLE matters ADD COLUMN agent_prefs TEXT /* JSON object: agent kind -> {option id: value id}; NULL = none, each agent's own defaults */;
