-- Tracks the listener loved on Last.fm. That list belongs to the listener's
-- profile there; the library's own star is a private flag on track_activity and
-- the two never move each other. A row exists only once Last.fm accepted the
-- love, so this table never claims more than it knows.
CREATE TABLE track_love (
    track_id INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    loved_at INTEGER NOT NULL
);
