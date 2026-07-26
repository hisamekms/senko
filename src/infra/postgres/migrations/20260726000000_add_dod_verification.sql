-- Add verification metadata to DoD items.
-- verification_type: how the item must be verified (static / execution / manual).
-- 'unspecified' is reserved for rows that existed before this migration.
-- verification_method: optional free-text procedure declared at registration.
-- verification_note: optional free-text record written when the item is checked.

ALTER TABLE task_definition_of_done
    ADD COLUMN verification_type TEXT NOT NULL DEFAULT 'unspecified',
    ADD COLUMN verification_method TEXT,
    ADD COLUMN verification_note TEXT;

ALTER TABLE contract_definition_of_done
    ADD COLUMN verification_type TEXT NOT NULL DEFAULT 'unspecified',
    ADD COLUMN verification_method TEXT,
    ADD COLUMN verification_note TEXT;
