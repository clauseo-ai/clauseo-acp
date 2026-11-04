-- Schema version 4: who set each thread's title, so the agent's title never replaces the lawyer's.
-- docs/decisions/0006-store-format.md gives the reason for the column and the title precedence.
-- Frozen: a later change is a new numbered file in this folder, never an edit here.

ALTER TABLE threads ADD COLUMN title_source TEXT /* RenameSource's JSON name; NULL with no title and on rows from before schema 4 */;
