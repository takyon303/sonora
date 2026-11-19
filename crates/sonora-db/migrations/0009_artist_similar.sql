-- Artists Last.fm calls similar, kept beside the library so a profile shows
-- them offline and a revisit costs no request.
CREATE TABLE artist_similar (
    artist_id INTEGER NOT NULL REFERENCES artists (id) ON DELETE CASCADE,
    position  INTEGER NOT NULL,
    name      TEXT    NOT NULL,
    PRIMARY KEY (artist_id, position)
);

-- Which artists have already been asked. An artist Last.fm knows nothing
-- about must not be looked up again on every visit.
CREATE TABLE artist_similar_fetched (
    artist_id  INTEGER PRIMARY KEY REFERENCES artists (id) ON DELETE CASCADE,
    fetched_at INTEGER NOT NULL
);
