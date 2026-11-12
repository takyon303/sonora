//! When a play becomes a scrobble.
//!
//! Last.fm only accepts a play once it has been listened to: a track shorter
//! than 30 seconds never counts, and everything else counts after half its
//! length or four minutes, whichever comes first. The watch below applies that
//! rule to the transport clock the UI already polls, and treats a forward jump
//! as a seek rather than as listening, so dragging the scrubber cannot
//! fabricate a scrobble.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::TrackId;

/// Tracks shorter than this are never scrobbled.
pub const MIN_TRACK_MS: i64 = 30_000;
/// Four minutes of a long track is enough.
pub const MIN_PLAY_MS: i64 = 240_000;
/// The largest step between two observations that still counts as playback;
/// anything bigger is a seek.
const MAX_STEP_MS: i64 = 2_000;
/// How far the position may jump backwards before it means "playing again".
const RESTART_MS: i64 = 1_000;

/// How much of a track has to be heard before it counts, or `None` when the
/// track is too short to scrobble at all.
pub fn play_threshold_ms(duration_ms: i64) -> Option<i64> {
    (duration_ms >= MIN_TRACK_MS).then(|| MIN_PLAY_MS.min(duration_ms / 2))
}

/// The current time as Unix seconds, which is the timestamp a scrobble carries.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// One play that has been listened to enough to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scrobble {
    pub track: TrackId,
    /// When the play began, in Unix seconds.
    pub started_at: i64,
    /// How much of the track was actually heard.
    pub played_ms: i64,
}

/// Follows the transport and reports a play once it deserves a scrobble.
#[derive(Debug, Default)]
pub struct ScrobbleWatch {
    track: Option<TrackId>,
    duration_ms: i64,
    played_ms: i64,
    last_position_ms: i64,
    started_at: i64,
    submitted: bool,
}

impl ScrobbleWatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one transport observation; `None` while nothing is playing.
    ///
    /// Returns a scrobble exactly once per play, when the threshold is crossed.
    pub fn observe(
        &mut self,
        track: Option<TrackId>,
        position_ms: i64,
        duration_ms: i64,
        now_unix: i64,
    ) -> Option<Scrobble> {
        let Some(track) = track.filter(|id| *id >= 0) else {
            *self = Self::default();
            return None;
        };
        let position_ms = position_ms.max(0);
        let position_delta = position_ms - self.last_position_ms;

        // Another track, or this one started over, is a new play: whatever was
        // accumulated belongs to the play that ended.
        if self.track != Some(track) || position_delta < -RESTART_MS {
            self.track = Some(track);
            self.duration_ms = duration_ms;
            self.played_ms = 0;
            self.submitted = false;
            self.started_at = now_unix - position_ms / 1_000;
            self.last_position_ms = position_ms;
            return None;
        }

        self.duration_ms = duration_ms;
        self.last_position_ms = position_ms;
        if (1..=MAX_STEP_MS).contains(&position_delta) {
            self.played_ms += position_delta;
        }
        if self.submitted {
            return None;
        }
        let threshold = play_threshold_ms(self.duration_ms)?;
        if self.played_ms < threshold {
            return None;
        }
        self.submitted = true;
        Some(Scrobble {
            track,
            started_at: self.started_at,
            played_ms: self.played_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Play a track from the start in one-second steps, collecting scrobbles.
    fn listen(
        watch: &mut ScrobbleWatch,
        track: TrackId,
        duration_ms: i64,
        until_ms: i64,
    ) -> Vec<Scrobble> {
        let mut scrobbles = Vec::new();
        let mut position = 0;
        let mut now = 1_700_000_000;
        while position <= until_ms {
            if let Some(scrobble) = watch.observe(Some(track), position, duration_ms, now) {
                scrobbles.push(scrobble);
            }
            position += 1_000;
            now += 1;
        }
        scrobbles
    }

    #[test]
    fn short_tracks_never_count() {
        assert_eq!(play_threshold_ms(29_999), None);
        assert_eq!(play_threshold_ms(30_000), Some(15_000));
        let mut watch = ScrobbleWatch::new();
        assert!(listen(&mut watch, 1, 20_000, 20_000).is_empty());
    }

    #[test]
    fn half_of_a_short_track_is_enough() {
        let mut watch = ScrobbleWatch::new();
        let scrobbles = listen(&mut watch, 7, 70_000, 70_000);
        assert_eq!(scrobbles.len(), 1, "{scrobbles:?}");
        let scrobble = &scrobbles[0];
        assert_eq!(scrobble.track, 7);
        assert_eq!(scrobble.played_ms, 35_000);
        assert_eq!(scrobble.started_at, 1_700_000_000);
    }

    #[test]
    fn four_minutes_of_a_long_track_is_enough() {
        assert_eq!(play_threshold_ms(3_600_000), Some(MIN_PLAY_MS));
        let mut watch = ScrobbleWatch::new();
        let scrobbles = listen(&mut watch, 3, 3_600_000, 300_000);
        assert_eq!(scrobbles.len(), 1, "{scrobbles:?}");
        assert_eq!(scrobbles[0].played_ms, 240_000);
        // Listening to the rest of the track does not report it again.
        let mut second = ScrobbleWatch::new();
        assert_eq!(
            listen(&mut second, 3, 3_600_000, 3_600_000).len(),
            1,
            "one play is reported once"
        );
    }

    #[test]
    fn a_seek_is_not_listening() {
        let mut watch = ScrobbleWatch::new();
        watch.observe(Some(5), 0, 600_000, 1_700_000_000);
        // Dragging to the end of the track must not count as 9 minutes of play.
        let scrobble = watch.observe(Some(5), 570_000, 600_000, 1_700_000_002);
        assert_eq!(scrobble, None);
        assert!(watch
            .observe(Some(5), 571_000, 600_000, 1_700_000_003)
            .is_none());
    }

    #[test]
    fn starting_the_same_track_again_is_a_new_play() {
        let mut watch = ScrobbleWatch::new();
        assert_eq!(listen(&mut watch, 2, 60_000, 40_000).len(), 1);
        // Repeat-one rewinds to the top with the same track id.
        assert!(watch.observe(Some(2), 0, 60_000, 1_700_000_060).is_none());
        assert!(watch
            .observe(Some(2), 1_000, 60_000, 1_700_000_061)
            .is_none());
        let again = listen(&mut watch, 2, 60_000, 40_000);
        assert_eq!(again.len(), 1, "{again:?}");
        assert_eq!(again[0].track, 2);
    }

    #[test]
    fn switching_tracks_starts_over() {
        let mut watch = ScrobbleWatch::new();
        watch.observe(Some(1), 0, 90_000, 1_700_000_000);
        watch.observe(Some(1), 40_000, 90_000, 1_700_000_040);
        // Skip to the next track; its first observation cannot carry the
        // previous track's listening time.
        assert!(watch.observe(Some(2), 0, 90_000, 1_700_000_041).is_none());
        assert!(watch
            .observe(Some(2), 1_000, 90_000, 1_700_000_042)
            .is_none());
    }

    #[test]
    fn stopping_playback_forgets_the_play() {
        let mut watch = ScrobbleWatch::new();
        watch.observe(Some(4), 0, 90_000, 1_700_000_000);
        watch.observe(Some(4), 40_000, 90_000, 1_700_000_040);
        watch.observe(None, 0, 0, 1_700_000_041);
        assert!(watch
            .observe(Some(4), 41_000, 90_000, 1_700_000_042)
            .is_none());
    }
}
