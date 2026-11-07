-- Sign-in to the Workbench (ADR 0028, ADR 0033). Forward-only.

-- Who owns a member name: the OIDC account that first signed in as it. A later account with the
-- same username (a renamed GitHub login taken by someone else) is refused, never merged.
CREATE TABLE members (
  member text PRIMARY KEY CHECK (member ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
  issuer text NOT NULL,
  subject text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

-- A sign-in in progress is not stored: it travels in a sealed cookie (rl-auth), so an
-- unauthenticated /auth/login writes nothing.

-- A signed-in browser. The cookie holds a random 256-bit token; only its SHA-256 is stored, so
-- reading this table doesn't let anyone sign in.
CREATE TABLE logins (
  token_hash bytea PRIMARY KEY CHECK (length(token_hash) = 32),
  member text NOT NULL REFERENCES members (member),
  created_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL
);

CREATE INDEX logins_expires ON logins (expires_at);
