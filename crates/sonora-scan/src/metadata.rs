//! Tag reading and writing with `lofty`.
//!
//! Sonora treats tags as the source of truth and only falls back to file names
//! for the fields a file genuinely lacks. Reading and writing live together
//! because they must agree on what a tag means: an edit is written, then read
//! back through [`extract`], so only what the file actually stores reaches the
//! library.

use std::fs::OpenOptions;
use std::path::Path;
use std::time::UNIX_EPOCH;

use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::{Accessor, ItemKey};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::Tag;
use sonora_core::{
    parse_artist_title, without_extension, AudioFormat, Error, Result, ScannedTrack,
};

/// Read a file's tags into a [`ScannedTrack`].
pub fn extract(path: &Path) -> Result<ScannedTrack> {
    let metadata = std::fs::metadata(path).map_err(|err| Error::io(path, err))?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    let tagged = Probe::open(path)
        .map_err(|err| Error::metadata(path, err.to_string()))?
        .read()
        .map_err(|err| Error::metadata(path, err.to_string()))?;

    let properties = tagged.properties();
    let duration_ms = properties.duration().as_millis() as u64;
    let codec = codec_label(path, tagged.file_type());

    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let mut title = tag
        .and_then(|tag| tag.title())
        .map(|value| value.to_string());
    let artist = tag
        .and_then(|tag| tag.artist())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let album = tag
        .and_then(|tag| tag.album())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let album_artist = tag
        .and_then(|tag| tag.get_string(ItemKey::AlbumArtist))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    // lofty 0.25 exposes the release date as a `Timestamp`; the bare year lives
    // inside it, so "1999", "1999-04" and "1999-04-12" all end up as 1999.
    let year = tag
        .and_then(|tag| tag.date())
        .map(|date| date.year as i32)
        .filter(|year| (1000..=3000).contains(year));
    let track_no = tag.and_then(|tag| tag.track());
    let disc_no = tag.and_then(|tag| tag.disk());
    let genres = tag
        .and_then(|tag| tag.genre())
        .map(|value| split_genres(&value))
        .unwrap_or_default();

    // ReplayGain. These are stored as text like "+2.3 dB" or "2.3 dB".
    // A missing or unparseable value leaves the field at None; ReplayGain is
    // never a scan-blocking problem.
    let replay_gain_track_db =
        parse_replay_gain_value(tag.and_then(|t| t.get_string(ItemKey::ReplayGainTrackGain)));
    let replay_gain_album_db =
        parse_replay_gain_value(tag.and_then(|t| t.get_string(ItemKey::ReplayGainAlbumGain)));
    let replay_gain_track_peak =
        parse_replay_gain_value(tag.and_then(|t| t.get_string(ItemKey::ReplayGainTrackPeak)));
    let replay_gain_album_peak =
        parse_replay_gain_value(tag.and_then(|t| t.get_string(ItemKey::ReplayGainAlbumPeak)));

    if title
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        title = None;
    }

    // Filename fallback. We only invent an artist when the file name itself
    // states one; otherwise the track keeps "Unknown Artist".
    let stem = without_extension(path);
    let (guessed_artist, guessed_title) = parse_artist_title(&stem);
    let title = title.unwrap_or(guessed_title);
    let artist = artist.or(guessed_artist);

    Ok(ScannedTrack {
        path: path.to_path_buf(),
        title,
        artist,
        album_artist,
        album,
        genres,
        track_no,
        disc_no,
        year,
        duration_ms,
        bitrate_kbps: properties.audio_bitrate(),
        sample_rate: properties.sample_rate(),
        channels: properties.channels(),
        codec,
        file_size: metadata.len(),
        modified_at,
        replay_gain_track_db,
        replay_gain_album_db,
        replay_gain_track_peak,
        replay_gain_album_peak,
    })
}

/// The tag values a person edited by hand.
///
/// Every text field is what the field should say afterwards: an empty string
/// means the tag is dropped, not blanked, so clearing a title leaves the file
/// with a title fallback from its name instead of an empty frame. Number
/// fields follow the same rule with `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagEdit {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub year: Option<u16>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
}

/// Write [`TagEdit`] into one file's primary tag.
///
/// The file is opened read-write and updated in place; nothing is renamed and
/// no audio data is touched. A format whose primary tag cannot be written
/// reports an error instead of silently dropping the edit, and callers re-read
/// the file afterwards so the library reflects what was stored.
pub fn write(path: &Path, edit: &TagEdit) -> Result<()> {
    let mut tagged = Probe::open(path)
        .map_err(|err| Error::metadata(path, err.to_string()))?
        .read()
        .map_err(|err| Error::metadata(path, err.to_string()))?;

    if tagged.primary_tag_mut().is_none() {
        // A file with no tag at all is common for rips; give it the tag type its
        // container expects rather than failing the edit.
        let tag_type = tagged.file_type().primary_tag_type();
        tagged.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged
        .primary_tag_mut()
        .ok_or_else(|| Error::metadata(path, "this file has no writable tag"))?;

    set_text(tag, &edit.title, Tag::set_title, Tag::remove_title);
    set_text(tag, &edit.artist, Tag::set_artist, Tag::remove_artist);
    set_text(tag, &edit.album, Tag::set_album, Tag::remove_album);

    let album_artist = edit.album_artist.trim();
    if album_artist.is_empty() {
        tag.remove_key(ItemKey::AlbumArtist);
    } else {
        tag.insert_text(ItemKey::AlbumArtist, album_artist.to_string());
    }

    match edit.year {
        Some(year) => tag.set_date(Timestamp {
            year,
            ..Default::default()
        }),
        None => tag.remove_date(),
    }
    match edit.track_no {
        Some(track) => tag.set_track(track),
        None => tag.remove_track(),
    }
    match edit.disc_no {
        Some(disc) => tag.set_disk(disc),
        None => tag.remove_disk(),
    }

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| Error::io(path, err))?;
    tagged
        .save_to(&mut file, WriteOptions::default())
        .map_err(|err| Error::metadata(path, err.to_string()))?;
    Ok(())
}

/// Set a text tag, or remove it when the field was left empty.
fn set_text(tag: &mut Tag, value: &str, set: fn(&mut Tag, String), remove: fn(&mut Tag)) {
    let value = value.trim();
    if value.is_empty() {
        remove(tag);
    } else {
        set(tag, value.to_string());
    }
}

/// Parse a ReplayGain tag value.
///
/// A gain is stored as text with a unit, `"+2.3 dB"` or `"-1.0dB"`; a peak is a
/// plain linear amplitude such as `"0.988553"`. A missing, empty or non-numeric
/// value yields `None`, because unreadable ReplayGain must never fail a scan.
fn parse_replay_gain_value(value: Option<&str>) -> Option<f32> {
    // Drop a trailing unit suffix such as "dB" in any case. Stripping by
    // character rather than by byte index keeps a malformed non-ASCII tag from
    // panicking on a slice boundary.
    let raw = value?
        .trim()
        .trim_end_matches(|character: char| character.is_ascii_alphabetic())
        .trim();
    raw.parse::<f32>().ok()
}

/// Prefer our own format label (stable, user-facing) over lofty's debug name.
fn codec_label(path: &Path, file_type: lofty::file::FileType) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(AudioFormat::from_extension)
        .map(|format| format.label().to_string())
        .unwrap_or_else(|| format!("{file_type:?}"))
}

/// Tags often pack several genres into one string ("Rock; Pop").
pub fn split_genres(value: &str) -> Vec<String> {
    value
        .split([';', '/', ','])
        .map(|genre| genre.trim())
        .filter(|genre| !genre.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_replay_gain_value, split_genres};

    #[test]
    fn parses_replay_gain_decibels() {
        assert_eq!(parse_replay_gain_value(Some("+2.3 dB")), Some(2.3));
        assert_eq!(parse_replay_gain_value(Some("-1.0DB")), Some(-1.0));
        assert_eq!(parse_replay_gain_value(Some(" 0.5 ")), Some(0.5));
        assert_eq!(parse_replay_gain_value(Some("0.0")), Some(0.0));
    }

    #[test]
    fn rejects_replay_gain_that_is_not_a_number() {
        assert_eq!(parse_replay_gain_value(Some("")), None);
        assert_eq!(parse_replay_gain_value(Some("   ")), None);
        assert_eq!(parse_replay_gain_value(Some("dB")), None);
        assert_eq!(parse_replay_gain_value(Some("loud")), None);
        assert_eq!(parse_replay_gain_value(None), None);
        // A malformed non-ASCII value must not panic on a slice boundary.
        assert_eq!(parse_replay_gain_value(Some("1.0€")), None);
    }

    #[test]
    fn splits_multi_value_genres() {
        assert_eq!(split_genres("Rock; Pop"), vec!["Rock", "Pop"]);
        assert_eq!(split_genres("Ambient / Drone"), vec!["Ambient", "Drone"]);
        assert_eq!(split_genres("  "), Vec::<String>::new());
    }
}
