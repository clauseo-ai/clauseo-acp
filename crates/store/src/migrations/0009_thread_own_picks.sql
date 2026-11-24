-- Schema version 9: the model and effort the lawyer last chose in each thread, kept apart from
-- what a session ran with, so a fallback model list never replaces them.
-- docs/decisions/0006-store-format.md gives the reason for the column and what NULL means.
-- Frozen: a later change is a new numbered file in this folder, never an edit here.

ALTER TABLE threads ADD COLUMN own_picks TEXT /* JSON object: {option id: value id}; NULL = none yet, and on rows from before schema 9 */;
