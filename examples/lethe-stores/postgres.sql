CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS lethe_memories (
    id text PRIMARY KEY,
    subject text NOT NULL,
    content text NOT NULL,
    embedding vector(3),
    retention_policy text NOT NULL,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS lethe_memories_subject_idx
    ON lethe_memories (subject);

CREATE INDEX IF NOT EXISTS lethe_memories_expires_at_idx
    ON lethe_memories (expires_at);

-- Aggregate request IDs are bound here before any configured store may delete.
-- Every coordinator process shares this ledger in the executable POC.
CREATE TABLE IF NOT EXISTS lethe_erasure_intents (
    request_id text PRIMARY KEY,
    subject_digest text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE IF NOT EXISTS lethe_erasure_requests (
    store text NOT NULL,
    request_id text NOT NULL,
    subject_digest text NOT NULL,
    result_json text,
    reservation_token text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (store, request_id)
);

-- Keep existing POC volumes forward-compatible with the reservation protocol.
ALTER TABLE lethe_erasure_requests
    ALTER COLUMN result_json DROP NOT NULL,
    ADD COLUMN IF NOT EXISTS reservation_token text;
