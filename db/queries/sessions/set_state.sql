UPDATE sessions SET state = $2, generation = $3, updated_at = $4 WHERE id = $1
