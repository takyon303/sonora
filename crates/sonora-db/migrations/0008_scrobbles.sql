-- Plays that have been listened to but not yet reported to Last.fm. The queue
-- is the source of truth, so a session that starts offline catches up later
-- instead of losing what was played.
CREATE TABLE scrobbles (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id  INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    played_at INTEGER NOT NULL,
    played_ms INTEGER NOT NULL,
    attempts  INTEGER NOT NULL DEFAULT 0,
    error     TEXT,
    added_at  INTEGER NOT NULL
);
