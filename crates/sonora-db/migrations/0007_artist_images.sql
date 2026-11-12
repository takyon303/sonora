-- Artist photos fetched from a keyless provider. Kept beside the library so a
-- profile shows its picture offline and a second visit costs no request.
CREATE TABLE artist_images (
    artist_id  INTEGER PRIMARY KEY REFERENCES artists (id) ON DELETE CASCADE,
    image      BLOB    NOT NULL,
    source     TEXT    NOT NULL,
    fetched_at INTEGER NOT NULL
);
