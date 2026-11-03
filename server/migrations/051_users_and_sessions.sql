-- +goose Up
-- Username and password sign-in (ADR 0042). One user exists for now, kept
-- in step with AUTH_USERNAME and AUTH_PASSWORD_HASH at startup; the table is
-- shaped for several. password_hash is an argon2id PHC string.
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- A signed-in browser. token_hash is the SHA-256 of the cookie's token, so
-- the database never holds a usable token. Times are Unix seconds;
-- last_seen_at is the last renewal, written at most once a day.
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id);

-- +goose Down
DROP INDEX IF EXISTS idx_sessions_user_id;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS users;
