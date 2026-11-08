-- Fails if sign-in left any row behind.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM logins) OR EXISTS (SELECT 1 FROM members) THEN
    RAISE EXCEPTION 'sign-in wrote rows';
  END IF;
END
$$;
