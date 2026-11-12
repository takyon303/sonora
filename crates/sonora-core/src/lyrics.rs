//! Turning a plain lyric into timed LRC, one tapped line at a time.
//!
//! A plain lyric is a block of text with no sense of when each line is sung.
//! Hand-syncing means playing the track and pressing a button as each line
//! starts, which records a timestamp per line. [`LyricSync`] is that recording:
//! it keeps the text, the stamps collected so far and the line the next press
//! belongs to, and it writes LRC only for the lines that were actually timed —
//! an untimed line is left out rather than guessed at.

/// Format a position as an LRC timestamp, e.g. `[01:05.40]`.
///
/// Centiseconds, not milliseconds: that is the precision LRC files carry and
/// what every player expects to read back.
pub fn lrc_timestamp(position_ms: u64) -> String {
    let minutes = position_ms / 60_000;
    let seconds = (position_ms % 60_000) / 1_000;
    let centis = (position_ms % 1_000) / 10;
    format!("[{minutes:02}:{seconds:02}.{centis:02}]")
}

/// Read the inside of an `[..]` tag, without the brackets.
///
/// Returns `None` for metadata tags (`ar:`, `ti:`, `offset:`) and for anything
/// that is not a `minutes:seconds` time, so a caller can ignore both without
/// mistaking one for a broken line.
pub fn parse_lrc_timestamp(tag: &str) -> Option<u64> {
    let (mm, rest) = tag.split_once(':')?;
    if mm.is_empty() || !mm.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let minutes: u64 = mm.parse().ok()?;
    let (ss, frac_ms) = if let Some((s, f)) = rest.split_once('.') {
        let sec: u64 = if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
            s.parse().ok()?
        } else {
            return None;
        };
        let digits: String = f.chars().take_while(|c| c.is_ascii_digit()).collect();
        let frac = match digits.len() {
            0 => 0,
            1 => digits.parse::<u64>().ok()? * 100,
            2 => digits.parse::<u64>().ok()? * 10,
            _ => digits[..3].parse::<u64>().ok()?,
        };
        (sec, frac)
    } else {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        (digits.parse().ok()?, 0)
    };
    Some(minutes * 60_000 + ss * 1_000 + frac_ms)
}

/// A lyric being timed line by line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LyricSync {
    lines: Vec<String>,
    stamps: Vec<Option<u64>>,
    /// The line the next stamp belongs to. Always points at an untimed line
    /// when one is left, so a press never overwrites a line already timed.
    cursor: usize,
}

impl LyricSync {
    /// Start from a plain lyric. Blank lines are dropped: there is nothing to
    /// hear on them, and a stamp would have no meaning.
    pub fn from_plain(text: &str) -> Self {
        let lines: Vec<String> = text
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        let stamps = vec![None; lines.len()];
        Self {
            lines,
            stamps,
            cursor: 0,
        }
    }

    /// Start from a lyric that already has times, so a bad tap can be fixed
    /// without retiming the whole song.
    pub fn from_timed(timed: &[(u64, String)]) -> Self {
        let mut lines = Vec::with_capacity(timed.len());
        let mut stamps = Vec::with_capacity(timed.len());
        for (at, text) in timed {
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            lines.push(text.to_string());
            stamps.push(Some(*at));
        }
        let cursor = next_untimed(&stamps, 0).unwrap_or(lines.len());
        Self {
            lines,
            stamps,
            cursor,
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn stamps(&self) -> &[Option<u64>] {
        &self.stamps
    }

    /// The line the next stamp belongs to.
    pub fn next(&self) -> Option<usize> {
        next_untimed(&self.stamps, self.cursor)
    }

    /// Lines that still have no time.
    pub fn untimed(&self) -> usize {
        self.stamps.iter().filter(|stamp| stamp.is_none()).count()
    }

    /// Time the current line at `at_ms`. Returns the line that was timed.
    ///
    /// Stamping out of order is allowed — a listener who seeks back is fixing a
    /// line, not breaking the file — and [`Self::to_lrc`] sorts on the way out.
    pub fn stamp(&mut self, at_ms: u64) -> Option<usize> {
        let index = self.next()?;
        self.stamps[index] = Some(at_ms);
        self.cursor = index + 1;
        Some(index)
    }

    /// Take the time off the line that was stamped last. Returns its index.
    pub fn undo(&mut self) -> Option<usize> {
        let index = self.stamps.iter().rposition(|stamp| stamp.is_some())?;
        self.stamps[index] = None;
        self.cursor = index;
        Some(index)
    }

    /// Take the time off one line, and put the cursor back on it.
    pub fn clear(&mut self, index: usize) -> bool {
        match self.stamps.get_mut(index) {
            Some(stamp @ Some(_)) => {
                *stamp = None;
                self.cursor = self.cursor.min(index);
                true
            }
            _ => false,
        }
    }

    /// Move the cursor to a line, so a tap can fix a specific one.
    pub fn set_next(&mut self, index: usize) {
        if index < self.lines.len() {
            self.cursor = index;
        }
    }

    /// The LRC text for everything timed so far, in playback order.
    ///
    /// Lines without a time are left out: saving half a song is honest, and a
    /// guessed timestamp would show the wrong words at the wrong moment.
    pub fn to_lrc(&self) -> String {
        let mut timed: Vec<(u64, usize)> = self
            .stamps
            .iter()
            .enumerate()
            .filter_map(|(index, stamp)| stamp.map(|at| (at, index)))
            .collect();
        timed.sort_by_key(|(at, index)| (*at, *index));

        let mut out = String::new();
        for (at, index) in timed {
            out.push_str(&lrc_timestamp(at));
            out.push(' ');
            out.push_str(&self.lines[index]);
            out.push('\n');
        }
        out
    }
}

fn next_untimed(stamps: &[Option<u64>], from: usize) -> Option<usize> {
    stamps
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, stamp)| stamp.is_none())
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_lyric_keeps_its_lines_and_drops_the_gaps() {
        let sync = LyricSync::from_plain("first line\n\n  second line  \n\n");
        assert_eq!(sync.lines(), ["first line", "second line"]);
        assert_eq!(sync.untimed(), 2);
        assert_eq!(sync.next(), Some(0));
    }

    #[test]
    fn stamping_times_lines_in_order_and_stops_when_they_run_out() {
        let mut sync = LyricSync::from_plain("one\ntwo");
        assert_eq!(sync.stamp(1_000), Some(0));
        assert_eq!(sync.stamp(2_500), Some(1));
        assert_eq!(sync.stamp(9_000), None, "no line is left to time");
        assert_eq!(sync.untimed(), 0);
        assert_eq!(sync.next(), None);
    }

    #[test]
    fn only_timed_lines_reach_the_file() {
        let mut sync = LyricSync::from_plain("one\ntwo\nthree");
        sync.stamp(2_000);
        sync.stamp(3_000);
        assert_eq!(
            sync.to_lrc(),
            "[00:02.00] one\n[00:03.00] two\n",
            "the untimed line is left out instead of guessed"
        );
    }

    #[test]
    fn a_line_timed_out_of_order_is_written_in_playback_order() {
        let mut sync = LyricSync::from_plain("one\ntwo");
        sync.stamp(5_000);
        sync.stamp(1_000);
        assert_eq!(sync.to_lrc(), "[00:01.00] two\n[00:05.00] one\n");
    }

    #[test]
    fn undo_frees_the_last_line_and_puts_the_cursor_back() {
        let mut sync = LyricSync::from_plain("one\ntwo");
        sync.stamp(1_000);
        sync.stamp(2_000);
        assert_eq!(sync.undo(), Some(1));
        assert_eq!(sync.stamps(), [Some(1_000), None]);
        assert_eq!(sync.next(), Some(1));
        assert_eq!(sync.undo(), Some(0));
        assert_eq!(sync.undo(), None);
    }

    #[test]
    fn a_timed_lyric_can_be_corrected_line_by_line() {
        let mut sync = LyricSync::from_timed(&[(500, "one".into()), (1_500, "two".into())]);
        assert_eq!(sync.untimed(), 0);
        assert_eq!(sync.next(), None);
        assert!(sync.clear(1));
        assert_eq!(sync.next(), Some(1));
        assert!(!sync.clear(1), "clearing an untimed line does nothing");
        assert_eq!(sync.stamp(1_400), Some(1));
        assert_eq!(sync.to_lrc(), "[00:00.50] one\n[00:01.40] two\n");
    }

    #[test]
    fn timestamps_read_and_write_the_same_precision() {
        assert_eq!(lrc_timestamp(0), "[00:00.00]");
        assert_eq!(lrc_timestamp(61_234), "[01:01.23]");
        for at in [0_u64, 999, 1_000, 61_234, 3_600_000] {
            let tag = lrc_timestamp(at);
            let inside = &tag[1..tag.len() - 1];
            assert_eq!(parse_lrc_timestamp(inside), Some(at / 10 * 10));
        }
    }

    #[test]
    fn metadata_tags_are_not_timestamps() {
        assert_eq!(parse_lrc_timestamp("ar:Someone"), None);
        assert_eq!(parse_lrc_timestamp("offset:-20"), None);
        assert_eq!(parse_lrc_timestamp(""), None);
    }
}
