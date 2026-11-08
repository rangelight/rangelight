-- The ticket's page in its tracker, for the Workbench's link (RAN-36). Forward-only.
-- Rows from before this migration keep '', which the check allows; new rows must name a url (the
-- default is dropped), and the api only writes https ones (rl-domain's Ticket::new checks them).
-- sqlx runs this file in one transaction, so the table stays locked until it commits; adding the
-- check NOT VALID and then validating it keeps the expand/contract shape, not a shorter lock.
ALTER TABLE sessions ADD COLUMN ticket_url text NOT NULL DEFAULT '';
ALTER TABLE sessions ALTER COLUMN ticket_url DROP DEFAULT;
ALTER TABLE sessions ADD CONSTRAINT sessions_ticket_url_https
  CHECK (ticket_url = '' OR ticket_url LIKE 'https://%') NOT VALID;
ALTER TABLE sessions VALIDATE CONSTRAINT sessions_ticket_url_https;
