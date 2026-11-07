-- Test fixture: make every outbox insert fail, after the command's other writes.
CREATE FUNCTION fail_outbox() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'injected failure';
END;
$$;
CREATE TRIGGER fail_outbox BEFORE INSERT ON outbox FOR EACH ROW EXECUTE FUNCTION fail_outbox();
