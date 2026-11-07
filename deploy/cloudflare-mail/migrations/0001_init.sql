-- AUTOINCREMENT guarantees a seq is never reused after retention deletes the
-- newest rows, so a client cursor can never point at a recycled id.
CREATE TABLE messages (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  r2_key TEXT NOT NULL UNIQUE,
  envelope_from TEXT NOT NULL,
  envelope_to TEXT NOT NULL,
  received_at INTEGER NOT NULL,
  size INTEGER NOT NULL CHECK (size >= 0 AND size <= 2097152),
  auth_failed INTEGER NOT NULL CHECK (auth_failed IN (0, 1))
);

CREATE INDEX idx_messages_received_at ON messages (received_at);
