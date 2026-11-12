-- The tracker $1 lists exactly tickets $2 as skipped now: every other skip of it is gone, the
-- tickets now valid (never to hear they were skipped) and those closed or unlabelled alike.
DELETE FROM ticket_skips WHERE tracker = $1 AND NOT (ticket_id = ANY($2::text[]))
